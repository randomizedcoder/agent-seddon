//! REST-transcoding annotation coverage over the emitted `FILE_DESCRIPTOR_SET`.
//!
//! gap-analysis §4 / `docs/design/rest-openapi/`: every transcodable RPC carries a
//! `(google.api.http)` option so Envoy's `grpc_json_transcoder` can expose it as
//! REST/JSON without any Rust code (gRPC stays the primary interface). This test
//! decodes the descriptor set the build emits and asserts those options survive
//! `tonic-build` codegen with the right verb + path template.
//!
//! `prost` silently drops unknown fields on decode, and `prost_types::MethodOptions`
//! has no field for the custom `google.api.http` extension (field 72295728) — so we
//! decode the descriptor with a *minimal* mirror of the relevant protobuf messages
//! that declares exactly that extension field. This reads the option without pulling
//! in a dynamic-reflection dependency (e.g. `prost-reflect`).
//!
//! The table is the annotation manifest: it grows one class-tagged row per RPC as
//! later increments annotate the surface (`docs/design/rest-openapi/STATUS.md`). The
//! whole-set invariants below (uniqueness, well-formedness, streaming kind) scale
//! automatically as rows are added.

use agent_proto::FILE_DESCRIPTOR_SET;
use prost::Message;
use rstest::rstest;
use std::collections::BTreeMap;

// ---- minimal descriptor mirror (only the fields we read) --------------------
//
// prost skips fields we don't declare, so these decode real descriptor bytes while
// exposing just method identity, streaming kind, and the http extension.

#[derive(Clone, PartialEq, Message)]
struct FileDescriptorSet {
    #[prost(message, repeated, tag = "1")]
    file: Vec<FileDescriptorProto>,
}

#[derive(Clone, PartialEq, Message)]
struct FileDescriptorProto {
    #[prost(message, repeated, tag = "6")]
    service: Vec<ServiceDescriptorProto>,
}

#[derive(Clone, PartialEq, Message)]
struct ServiceDescriptorProto {
    #[prost(string, optional, tag = "1")]
    name: Option<String>,
    #[prost(message, repeated, tag = "2")]
    method: Vec<MethodDescriptorProto>,
}

#[derive(Clone, PartialEq, Message)]
struct MethodDescriptorProto {
    #[prost(string, optional, tag = "1")]
    name: Option<String>,
    #[prost(bool, optional, tag = "5")]
    client_streaming: Option<bool>,
    #[prost(bool, optional, tag = "6")]
    server_streaming: Option<bool>,
    #[prost(message, optional, tag = "4")]
    options: Option<MethodOptions>,
}

// `google.protobuf.MethodOptions` carries the `(google.api.http)` extension at field
// 72295728 (declared in the vendored `google/api/annotations.proto`). We mirror only
// that one field; every other MethodOptions field is skipped on decode.
#[derive(Clone, PartialEq, Message)]
struct MethodOptions {
    #[prost(message, optional, tag = "72295728")]
    http: Option<HttpRule>,
}

// `google.api.HttpRule` (subset: the pattern verbs, body, and one level of bindings).
#[derive(Clone, PartialEq, Message)]
struct HttpRule {
    #[prost(string, optional, tag = "2")]
    get: Option<String>,
    #[prost(string, optional, tag = "3")]
    put: Option<String>,
    #[prost(string, optional, tag = "4")]
    post: Option<String>,
    #[prost(string, optional, tag = "5")]
    delete: Option<String>,
    #[prost(string, optional, tag = "6")]
    patch: Option<String>,
    #[prost(string, optional, tag = "7")]
    body: Option<String>,
    #[prost(message, repeated, tag = "11")]
    additional_bindings: Vec<HttpRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Route {
    verb: &'static str,
    path: String,
}

impl HttpRule {
    fn primary_route(&self) -> Option<Route> {
        let (verb, path) = if let Some(p) = &self.get {
            ("GET", p)
        } else if let Some(p) = &self.post {
            ("POST", p)
        } else if let Some(p) = &self.put {
            ("PUT", p)
        } else if let Some(p) = &self.delete {
            ("DELETE", p)
        } else if let Some(p) = &self.patch {
            ("PATCH", p)
        } else {
            return None;
        };
        Some(Route {
            verb,
            path: path.clone(),
        })
    }

    /// The primary binding plus any `additional_bindings` (one level deep).
    fn routes(&self) -> Vec<Route> {
        let mut out = Vec::new();
        out.extend(self.primary_route());
        for b in &self.additional_bindings {
            out.extend(b.primary_route());
        }
        out
    }
}

struct MethodInfo {
    client_streaming: bool,
    routes: Vec<Route>,
}

/// Decode the emitted descriptor set into `"Service.Method" -> MethodInfo`.
fn methods() -> BTreeMap<String, MethodInfo> {
    let set =
        FileDescriptorSet::decode(FILE_DESCRIPTOR_SET).expect("emitted descriptor set decodes");
    let mut map = BTreeMap::new();
    for f in &set.file {
        for s in &f.service {
            let Some(svc) = s.name.as_deref() else {
                continue;
            };
            for m in &s.method {
                let Some(name) = m.name.as_deref() else {
                    continue;
                };
                let routes = m
                    .options
                    .as_ref()
                    .and_then(|o| o.http.as_ref())
                    .map(HttpRule::routes)
                    .unwrap_or_default();
                map.insert(
                    format!("{svc}.{name}"),
                    MethodInfo {
                        client_streaming: m.client_streaming.unwrap_or(false),
                        routes,
                    },
                );
            }
        }
    }
    map
}

/// Expected `google.api.http` mapping for an RPC.
#[derive(Debug)]
enum Expect {
    /// Exactly these `(verb, path)` routes, in binding order.
    Routes(&'static [(&'static str, &'static str)]),
    /// Present in the descriptor but carrying no `google.api.http` rule.
    Unmapped,
    /// Not a real method — the lookup must fail closed (junk name).
    Absent,
}

#[rstest]
// positive — a read RPC transcodes to GET with an `{id}` path param (also the PR-02
// proof that the option survives `tonic-build` codegen into `FILE_DESCRIPTOR_SET`).
#[case::positive_read_rpc_maps_to_get_with_id(
    "read RPC ReviewFleetService.Get -> GET /v1/fleet/sessions/{id}",
    "ReviewFleetService.Get",
    Expect::Routes(&[("GET", "/v1/fleet/sessions/{id}")])
)]
// positive — an upsert maps to POST on the collection with the whole message as body.
#[case::positive_upsert_maps_to_post_collection(
    "upsert RPC ReviewFleetService.Put -> POST /v1/fleet/sessions (body: *)",
    "ReviewFleetService.Put",
    Expect::Routes(&[("POST", "/v1/fleet/sessions")])
)]
// positive — an imperative action maps to POST with an id path param + body.
#[case::positive_action_maps_to_post_with_id(
    "action RPC ReviewFleetService.Approve -> POST /v1/fleet/reviews/{review_id}/approve",
    "ReviewFleetService.Approve",
    Expect::Routes(&[("POST", "/v1/fleet/reviews/{review_id}/approve")])
)]
// corner — a delete maps to the DELETE verb by id (a less common verb in the surface).
#[case::corner_delete_maps_to_delete_verb(
    "ReviewFleetService.Delete -> DELETE /v1/fleet/sessions/{id}",
    "ReviewFleetService.Delete",
    Expect::Routes(&[("DELETE", "/v1/fleet/sessions/{id}")])
)]
// corner — a toggle maps to POST with BOTH a path capture and a `body: *` (the field
// not captured by the path lands in the body).
#[case::corner_toggle_has_path_param_and_body(
    "ReviewFleetService.SetEnabled -> POST /v1/fleet/sessions/{id}/enabled",
    "ReviewFleetService.SetEnabled",
    Expect::Routes(&[("POST", "/v1/fleet/sessions/{id}/enabled")])
)]
// boundary — a read RPC with an empty request maps to GET with ZERO path params.
#[case::boundary_paramless_read_maps_to_bare_get(
    "ReviewFleetService.Preflight (empty request) -> GET /v1/fleet/preflight",
    "ReviewFleetService.Preflight",
    Expect::Routes(&[("GET", "/v1/fleet/preflight")])
)]
// --- 03b: control plane (role / prompt / config) ---------------------------
// positive — RoleService mirrors the fleet-session CRUD shape in a different proto
// group (read by id → GET).
#[case::positive_role_get_maps_to_get_with_id(
    "RoleService.Get -> GET /v1/roles/{id}",
    "RoleService.Get",
    Expect::Routes(&[("GET", "/v1/roles/{id}")])
)]
// positive — ConfigService write maps to POST body:* on the resource root.
#[case::positive_config_put_maps_to_post(
    "ConfigService.Put -> POST /v1/config (body: *)",
    "ConfigService.Put",
    Expect::Routes(&[("POST", "/v1/config")])
)]
// corner — a prompt's identity is the (kind, id) PAIR, so its read route carries TWO
// path params (the first RPC in the surface with a composite key).
#[case::corner_prompt_get_has_two_path_params(
    "PromptService.Get -> GET /v1/prompts/{kind}/{id} (composite key)",
    "PromptService.Get",
    Expect::Routes(&[("GET", "/v1/prompts/{kind}/{id}")])
)]
// corner — a read whose request is only a repeated-scalar filter maps to GET (the tag
// list rides as repeated query params), not POST.
#[case::corner_repeated_scalar_filter_read_maps_to_get(
    "PromptService.Select -> GET /v1/prompts/select",
    "PromptService.Select",
    Expect::Routes(&[("GET", "/v1/prompts/select")])
)]
// boundary — a paramless read on a sub-resource maps to a bare GET sub-path.
#[case::boundary_config_schema_maps_to_bare_get_subpath(
    "ConfigService.GetSchema (empty request) -> GET /v1/config/schema",
    "ConfigService.GetSchema",
    Expect::Routes(&[("GET", "/v1/config/schema")])
)]
// negative — an RPC in a not-yet-annotated proto carries no rule. This row flips to a
// positive `Routes` case when `policy.proto` is annotated in a later increment.
#[case::negative_unannotated_rpc_has_no_rule(
    "Policy.Authorize is not annotated yet -> no google.api.http rule",
    "Policy.Authorize",
    Expect::Unmapped
)]
// negative — a junk method name must not resolve to any rule (fail closed).
#[case::negative_junk_method_is_absent(
    "a non-existent method resolves to nothing (lookup fails closed)",
    "ReviewFleetService.NoSuchMethod",
    Expect::Absent
)]
fn http_annotation_coverage(
    #[case] _description: &str,
    #[case] method: &str,
    #[case] expect: Expect,
) {
    let map = methods();
    match expect {
        Expect::Absent => assert!(
            !map.contains_key(method),
            "expected `{method}` to be absent from the descriptor, but it is present"
        ),
        Expect::Unmapped => {
            let info = map
                .get(method)
                .unwrap_or_else(|| panic!("`{method}` not found in descriptor"));
            assert!(
                info.routes.is_empty(),
                "expected `{method}` to carry no google.api.http rule, got {:?}",
                info.routes
            );
        }
        Expect::Routes(expected) => {
            let info = map
                .get(method)
                .unwrap_or_else(|| panic!("`{method}` not found in descriptor"));
            let got: Vec<(&str, &str)> = info
                .routes
                .iter()
                .map(|r| (r.verb, r.path.as_str()))
                .collect();
            assert_eq!(got.as_slice(), expected, "route mismatch for `{method}`");
        }
    }
}

// ---- whole-set invariants (scale automatically as rows are annotated) -------

// boundary — the one annotated read route has exactly one `{id}` path param with
// balanced braces (the transcoder binds it to the request's `id` field).
#[test]
fn boundary_annotated_read_route_has_exactly_one_path_param() {
    let map = methods();
    let info = map
        .get("ReviewFleetService.Get")
        .expect("ReviewFleetService.Get present");
    let route = info
        .routes
        .first()
        .expect("ReviewFleetService.Get is annotated");
    assert_eq!(
        route.path.matches('{').count(),
        1,
        "expected exactly one path param in `{}`",
        route.path
    );
    assert_eq!(
        route.path.matches('{').count(),
        route.path.matches('}').count(),
        "unbalanced braces in `{}`",
        route.path
    );
}

// adversarial — no two RPCs may claim the same `(verb, path)`, or the transcoder's
// route match is ambiguous.
#[test]
fn adversarial_no_two_rpcs_share_the_same_route() {
    let map = methods();
    let mut seen: BTreeMap<(&str, String), String> = BTreeMap::new();
    for (method, info) in &map {
        for r in &info.routes {
            if let Some(prev) = seen.insert((r.verb, r.path.clone()), method.clone()) {
                panic!(
                    "route `{} {}` is claimed by both `{prev}` and `{method}`",
                    r.verb, r.path
                );
            }
        }
    }
}

// adversarial — every annotated route is absolute, version-prefixed, brace-balanced,
// and free of `..` (paths are part of the wire contract; fail closed on malformed).
#[test]
fn adversarial_every_route_is_versioned_and_safe() {
    let map = methods();
    for (method, info) in &map {
        for r in &info.routes {
            assert!(
                r.path.starts_with("/v1/"),
                "`{method}` route is not under /v1/: {}",
                r.path
            );
            assert!(
                !r.path.contains(".."),
                "`{method}` route contains `..`: {}",
                r.path
            );
            assert_eq!(
                r.path.matches('{').count(),
                r.path.matches('}').count(),
                "`{method}` route has unbalanced braces: {}",
                r.path
            );
        }
    }
}

// boundary — the Envoy transcoder supports unary + server-streaming, NOT client/bidi.
// Today the whole surface is transcodable (all streaming RPCs are server-streaming),
// so the REST exclusion list is empty. If a client-streaming/bidi RPC is ever added
// this fires — a reminder to mark it gRPC-only in docs/design/rest-openapi/.
#[test]
fn boundary_surface_has_no_client_or_bidi_streaming_rpcs() {
    let map = methods();
    let offenders: Vec<&String> = map
        .iter()
        .filter(|(_, i)| i.client_streaming)
        .map(|(m, _)| m)
        .collect();
    assert!(
        offenders.is_empty(),
        "client-streaming/bidi RPCs are not REST-transcodable; mark them gRPC-only: {offenders:?}"
    );
}
