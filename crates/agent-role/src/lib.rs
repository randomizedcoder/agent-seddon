//! `agent-role` — the [`RoleRegistry`](agent_core::RoleRegistry) backed by the
//! shared transactional config store (config design C34 / increment C1b).
//!
//! Operator-defined RBAC role *cards* persist onto any
//! [`agent_config_store::Backend`] (memory, file, sqlite, or postgres) as the
//! prost-encoded `pb::RoleCard` blob — the same store spine the A3* domain
//! convergences use. The three **built-in** roles are never stored; they seed the
//! catalog and a card may not reuse one of their ids (enforced by
//! [`agent_core::RoleCard::validate`]). [`agent_core::load_catalog`] folds these
//! stored cards atop the built-ins into the live catalog the RBAC gate authorizes
//! against.
//!
//! **Untrusted input, fail closed.** Ids reach the backend only after
//! `safe_segment` (via the store's own boundary checks and the card's `validate`);
//! blobs decode-then-`validate` on read, so an out-of-band-tampered or
//! wrong-schema row fails closed at the seam rather than granting anything.

#[cfg(feature = "role-store")]
mod store;
#[cfg(feature = "role-store")]
pub use store::{StoreRoles, DEFAULT_TENANT};
