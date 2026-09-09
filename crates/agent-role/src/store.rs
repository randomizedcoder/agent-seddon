//! `StoreRoles` — the [`RoleRegistry`] over a shared [`agent_config_store::Backend`].

use std::sync::Arc;

use agent_config_store::{Backend, Write, DEFAULT_MAX_CARDS_PER_TENANT};
use agent_core::{safe_segment, Error, Result, RoleCard, RoleRegistry};
use agent_proto::pb;
use async_trait::async_trait;
use prost::Message;

/// The collection holding one card per operator-defined role (id = the role name).
const ROLES: &str = "roles";
/// The default single-tenant scope. Per-tenant scoping (a verified tenant per call)
/// arrives with the per-tenant plane (config C35 / increment C2); until then the
/// role store is one un-namespaced control plane under this key.
pub const DEFAULT_TENANT: &str = "local";

/// A [`RoleRegistry`] persisted on a shared [`Backend`]. Cheap to clone (an `Arc`
/// handle plus the tenant key).
pub struct StoreRoles {
    backend: Arc<dyn Backend>,
    tenant: String,
}

impl StoreRoles {
    /// A role store over `backend` under the default single-tenant scope.
    pub fn new(backend: Arc<dyn Backend>) -> Self {
        Self {
            backend,
            tenant: DEFAULT_TENANT.to_string(),
        }
    }

    /// A role store scoped to an explicit tenant (config C2 will call this per
    /// verified identity). The tenant is `safe_segment`-gated — a hostile tenant is
    /// rejected at construction, never persisted or turned into a key.
    pub fn with_tenant(backend: Arc<dyn Backend>, tenant: &str) -> Result<Self> {
        if !safe_segment(tenant) {
            return Err(Error::Config(format!("invalid tenant `{tenant}`")));
        }
        Ok(Self {
            backend,
            tenant: tenant.to_string(),
        })
    }

    /// Decode a stored blob into a validated core card. A blob that cannot be
    /// decoded, carries an out-of-set action/resource, or fails structural
    /// validation is a fail-closed error (an out-of-band tamper), never a partial card.
    fn decode(blob: &[u8]) -> Result<RoleCard> {
        let wire = pb::RoleCard::decode(blob)
            .map_err(|e| Error::Config(format!("stored role decode: {e}")))?;
        let card = RoleCard::try_from(wire)
            .map_err(|e| Error::Config(format!("stored role decode: {e}")))?;
        card.validate()?;
        Ok(card)
    }
}

#[async_trait]
impl RoleRegistry for StoreRoles {
    async fn list(&self) -> Result<Vec<RoleCard>> {
        self.backend
            .list(ROLES, &self.tenant)
            .await?
            .iter()
            .map(|blob| Self::decode(blob))
            .collect()
    }

    async fn get(&self, id: &str) -> Result<RoleCard> {
        if !safe_segment(id) {
            return Err(Error::Config(format!("invalid role id `{id}`")));
        }
        match self.backend.get(ROLES, &self.tenant, id).await? {
            Some(blob) => Self::decode(&blob),
            // The `not found` prefix is the seam contract (the wire maps it to NotFound).
            None => Err(Error::Config(format!("not found: role card `{id}`"))),
        }
    }

    async fn put(&self, card: RoleCard) -> Result<RoleCard> {
        // Fail-closed: a path-unsafe or reserved-built-in id is rejected before any
        // write (the id may become a storage key).
        card.validate()?;
        let id = card.id.clone();
        let is_new = self.backend.get(ROLES, &self.tenant, &id).await?.is_none();
        if is_new && self.backend.count(ROLES, &self.tenant).await? >= DEFAULT_MAX_CARDS_PER_TENANT
        {
            return Err(Error::Config(format!(
                "roles is full ({DEFAULT_MAX_CARDS_PER_TENANT} cards for tenant `{}`)",
                self.tenant
            )));
        }
        let blob = pb::RoleCard::from(card.clone()).encode_to_vec();
        self.backend
            .apply(&[
                Write::EnsureTenant {
                    tenant: self.tenant.clone(),
                },
                Write::Put {
                    collection: ROLES,
                    tenant: self.tenant.clone(),
                    id,
                    blob,
                },
            ])
            .await?;
        Ok(card)
    }

    async fn delete(&self, id: &str) -> Result<bool> {
        if !safe_segment(id) {
            return Err(Error::Config(format!("invalid role id `{id}`")));
        }
        let existed = self.backend.get(ROLES, &self.tenant, id).await?.is_some();
        self.backend
            .apply(&[Write::Delete {
                collection: ROLES,
                tenant: self.tenant.clone(),
                id: id.to_string(),
            }])
            .await?;
        Ok(existed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_config_store::MemoryBackend;
    use agent_core::{
        authorize, load_catalog, Action, Resource, ResourceType, RoleCard, RolePermissions,
        VerifiedPrincipal, ROLE_OPERATOR,
    };
    use rstest::rstest;

    fn store() -> StoreRoles {
        StoreRoles::new(Arc::new(MemoryBackend::new()))
    }

    fn card(id: &str, perms: RolePermissions) -> RoleCard {
        RoleCard {
            id: id.to_string(),
            crosses_tenants: false,
            permissions: perms,
        }
    }

    // desc: a put→get round-trips the stored card verbatim.
    #[tokio::test]
    async fn positive_put_get_roundtrip() {
        let s = store();
        let c = card(
            "reviewer",
            RolePermissions::Pairs(vec![(Action::Approve, ResourceType::Fleet)]),
        );
        let stored = s.put(c.clone()).await.expect("put");
        assert_eq!(stored, c);
        assert_eq!(s.get("reviewer").await.expect("get"), c);
        let all = s.list().await.expect("list");
        assert_eq!(all, vec![c]);
    }

    // desc: a persisted role, folded into the catalog, grants exactly its permission.
    #[tokio::test]
    async fn positive_persisted_role_grants_action() {
        let s = store();
        s.put(card(
            "approver",
            RolePermissions::Pairs(vec![(Action::Approve, ResourceType::Fleet)]),
        ))
        .await
        .expect("put");
        let cat = load_catalog(&s).await.expect("catalog");
        let p = VerifiedPrincipal {
            tenant: "local".into(),
            subject: "op".into(),
            roles: vec!["approver".into()],
        };
        // The granted pair is allowed; a different action on it is not.
        assert!(authorize(
            &cat,
            &p,
            Action::Approve,
            &Resource::new(ResourceType::Fleet, "local")
        )
        .is_allowed());
        assert!(!authorize(
            &cat,
            &p,
            Action::Write,
            &Resource::new(ResourceType::Fleet, "local")
        )
        .is_allowed());
        // The built-ins survive alongside the persisted card.
        assert!(cat.get(ROLE_OPERATOR).is_some());
    }

    // desc: get of an unknown id is a `not found` error (maps to NotFound on the wire).
    #[tokio::test]
    async fn negative_missing_role() {
        let s = store();
        let err = s.get("ghost").await.expect_err("must be absent");
        // The inner message carries the `not found` prefix (the seam contract that the
        // wire maps to NotFound); Display prefixes the variant name ("config error: ").
        assert!(err.to_string().contains("not found"), "got: {err}");
    }

    // desc: delete is idempotent — true when it existed, false when absent.
    #[tokio::test]
    async fn negative_delete_absent_is_false() {
        let s = store();
        assert!(!s.delete("ghost").await.expect("delete absent"));
        s.put(card("temp", RolePermissions::All))
            .await
            .expect("put");
        assert!(s.delete("temp").await.expect("delete present"));
        assert!(!s.delete("temp").await.expect("delete again"));
    }

    // boundary: an admin (All) card grants every action on every resource.
    #[tokio::test]
    async fn boundary_all_grants_every_action() {
        let s = store();
        s.put(card("super", RolePermissions::All))
            .await
            .expect("put");
        let cat = load_catalog(&s).await.expect("catalog");
        let p = VerifiedPrincipal {
            tenant: "local".into(),
            subject: "op".into(),
            roles: vec!["super".into()],
        };
        for (a, r) in [
            (Action::Write, ResourceType::Config),
            (Action::Delete, ResourceType::Scheduler),
        ] {
            assert!(authorize(&cat, &p, a, &Resource::new(r, "local")).is_allowed());
        }
    }

    // corner: an empty-permissions card grants nothing.
    #[tokio::test]
    async fn corner_empty_permissions_denies_all() {
        let s = store();
        s.put(card("noop", RolePermissions::ActionsOnAll(vec![])))
            .await
            .expect("put");
        let cat = load_catalog(&s).await.expect("catalog");
        let p = VerifiedPrincipal {
            tenant: "local".into(),
            subject: "op".into(),
            roles: vec!["noop".into()],
        };
        assert!(!authorize(
            &cat,
            &p,
            Action::Read,
            &Resource::new(ResourceType::Config, "local")
        )
        .is_allowed());
    }

    // adversarial: a card reusing a reserved built-in id is rejected (no shadowing).
    #[rstest]
    #[case::operator("operator")]
    #[case::org_admin("org_admin")]
    #[case::reader("reader")]
    #[tokio::test]
    async fn adversarial_reserved_id_rejected(#[case] id: &str) {
        let s = store();
        let err = s
            .put(card(id, RolePermissions::All))
            .await
            .expect_err("reserved");
        assert!(err.to_string().contains("reserved"), "got: {err}");
    }

    // adversarial: a path-traversal / separator id is rejected before any write.
    #[rstest]
    #[case::traversal("../etc")]
    #[case::separator("a/b")]
    #[case::empty("")]
    #[tokio::test]
    async fn adversarial_hostile_id_rejected(#[case] id: &str) {
        let s = store();
        assert!(s.put(card(id, RolePermissions::All)).await.is_err());
    }

    // adversarial: an unknown action STRING never survives to the store — it is
    // rejected at the wire→core boundary, so no card with a bogus action can be put.
    #[tokio::test]
    async fn adversarial_unknown_action_string_rejected() {
        // The wire carries the raw string; TryFrom is the fail-closed boundary.
        let wire = pb::RoleCard {
            id: "bad".into(),
            crosses_tenants: false,
            all: false,
            actions_on_all: vec!["superwrite".into()],
            pairs: vec![],
        };
        let parsed = agent_core::RoleCard::try_from(wire);
        assert!(parsed.is_err(), "unknown action must be rejected");
    }
}

// The Postgres arm exercised against a REAL server — the tier `nix flake check`
// cannot host (no DB in the sandbox). `#[ignore]`-gated and run single-threaded by
// the `pg-integration` harness, which sets `AGENT_CONFIG_STORE_TEST_DSN`. A
// dedicated tenant keeps the run isolated without a global TRUNCATE.
#[cfg(all(test, feature = "role-store-postgres"))]
mod pg_tests {
    use super::*;
    use agent_config_store::PgBackend;
    use agent_core::{
        authorize, load_catalog, Action, Resource, ResourceType, RolePermissions, VerifiedPrincipal,
    };

    const IT_TENANT: &str = "c1b_role_it";

    async fn pg_roles() -> StoreRoles {
        let dsn = std::env::var("AGENT_CONFIG_STORE_TEST_DSN")
            .expect("AGENT_CONFIG_STORE_TEST_DSN must be set by the pg-integration harness");
        let backend = PgBackend::connect(&dsn, 4, true)
            .await
            .expect("connect postgres + ensure schema");
        let roles = StoreRoles::with_tenant(Arc::new(backend), IT_TENANT).expect("tenant");
        // Clean slate for this tenant (idempotent across re-runs).
        for c in roles.list().await.expect("list") {
            roles.delete(&c.id).await.expect("cleanup delete");
        }
        roles
    }

    // desc (postgres, live): a card round-trips over a real server and, folded into
    // the catalog, grants exactly its permission — the behaviour-preservation proof
    // for the converged postgres tier.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn positive_pg_roundtrip_and_grants() {
        let roles = pg_roles().await;
        let c = RoleCard {
            id: "pg_reviewer".into(),
            crosses_tenants: false,
            permissions: RolePermissions::Pairs(vec![(Action::Approve, ResourceType::Fleet)]),
        };
        roles.put(c.clone()).await.expect("put");
        assert_eq!(roles.get("pg_reviewer").await.expect("get"), c);

        let cat = load_catalog(&roles).await.expect("catalog");
        let p = VerifiedPrincipal {
            tenant: IT_TENANT.into(),
            subject: "op".into(),
            roles: vec!["pg_reviewer".into()],
        };
        assert!(authorize(
            &cat,
            &p,
            Action::Approve,
            &Resource::new(ResourceType::Fleet, IT_TENANT)
        )
        .is_allowed());
        assert!(roles.delete("pg_reviewer").await.expect("delete"));
    }

    // adversarial (postgres, live): a hostile id never mutates the store, and a
    // reserved built-in id is refused, over the real server.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn adversarial_pg_hostile_and_reserved_rejected() {
        let roles = pg_roles().await;
        for id in ["../../etc/passwd", "a/b", ""] {
            assert!(roles.get(id).await.is_err(), "get {id:?}");
            assert!(roles.delete(id).await.is_err(), "delete {id:?}");
        }
        let reserved = RoleCard {
            id: "operator".into(),
            crosses_tenants: false,
            permissions: RolePermissions::All,
        };
        assert!(roles.put(reserved).await.is_err(), "reserved id must fail");
    }
}
