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
// --- 03c: registries (forge / transport / provider-router) ------------------
// positive — ForgeRegistryService is a third CRUD mirror in a distinct proto group.
#[case::positive_forge_get_maps_to_get_with_id(
    "ForgeRegistryService.Get -> GET /v1/forges/{id}",
    "ForgeRegistryService.Get",
    Expect::Routes(&[("GET", "/v1/forges/{id}")])
)]
// corner — the provider registry's upstream toggle nests under the /upstreams
// collection (a path capture deeper than the resource root).
#[case::corner_upstream_enable_nested_under_collection(
    "ProviderRegistryService.Enable -> POST /v1/router/upstreams/{id}/enable",
    "ProviderRegistryService.Enable",
    Expect::Routes(&[("POST", "/v1/router/upstreams/{id}/enable")])
)]
// corner — Route is read-only introspection but its request carries a nested
// `RouteHint`, so it maps to POST body:* (the nested-body read convention), not GET.
#[case::corner_route_introspection_maps_to_post(
    "ProviderRegistryService.Route -> POST /v1/router/route (nested-body read)",
    "ProviderRegistryService.Route",
    Expect::Routes(&[("POST", "/v1/router/route")])
)]
// --- 03d: code intelligence (repo / search / ast) --------------------------
// positive — an object read is revision-addressed, and a revision/path may contain
// `/`, so it rides as a query param: GET with ZERO path params (contrast the
// `safe_segment` id reads, which DO capture `{id}`).
#[case::positive_repo_readfile_revision_is_query_param(
    "RepoService.ReadFile -> GET /v1/repo/file (revision/path are query params, not captures)",
    "RepoService.ReadFile",
    Expect::Routes(&[("GET", "/v1/repo/file")])
)]
// corner — the FIRST server-streaming RPC in the manifest: the transcoder supports it
// (chunked JSON), so it IS annotated. A reindex is a side-effect → POST body:*.
#[case::corner_server_streaming_reindex_is_annotated_post(
    "SearchService.Reindex (server-streaming) -> POST /v1/search/reindex",
    "SearchService.Reindex",
    Expect::Routes(&[("POST", "/v1/search/reindex")])
)]
// corner — a structural read that names its target by a nested `SymbolRef` message
// maps to POST body:* (nested-body-read convention), though it is side-effect-free.
#[case::corner_ast_callers_nested_body_read_maps_to_post(
    "AstService.Callers -> POST /v1/ast/callers (nested SymbolRef in the request)",
    "AstService.Callers",
    Expect::Routes(&[("POST", "/v1/ast/callers")])
)]
// corner — a structural read whose request is ONLY repeated-scalar (`changed` paths)
// stays GET (the paths ride as repeated query params) — the direct contrast to the
// nested-body Callers row above.
#[case::corner_ast_blast_radius_repeated_scalar_read_maps_to_get(
    "AstService.BlastRadius -> GET /v1/ast/blast-radius (repeated-scalar filter)",
    "AstService.BlastRadius",
    Expect::Routes(&[("GET", "/v1/ast/blast-radius")])
)]
// corner — list + create share ONE collection path, disambiguated only by verb (GET
// lists, POST creates); the uniqueness invariant treats `(verb, path)` as the key so
// they don't collide. This row locks the GET half; `WorktreeAdd` locks the POST half.
#[case::corner_worktree_list_shares_collection_path_via_verb(
    "RepoService.WorktreeList -> GET /v1/repo/worktrees (same path as POST WorktreeAdd)",
    "RepoService.WorktreeList",
    Expect::Routes(&[("GET", "/v1/repo/worktrees")])
)]
#[case::corner_worktree_add_shares_collection_path_via_verb(
    "RepoService.WorktreeAdd -> POST /v1/repo/worktrees (same path as GET WorktreeList)",
    "RepoService.WorktreeAdd",
    Expect::Routes(&[("POST", "/v1/repo/worktrees")])
)]
// --- 03e: sessions (session / session_registry / agent_session / scheduler) --
// corner — a session is keyed by the `(user, session_id)` PAIR, so Close nests as a
// child collection: a DELETE carrying TWO path params (composite key + DELETE, deeper
// than the single-param prompt read).
#[case::corner_close_composite_key_nested_delete(
    "SessionRegistryService.Close -> DELETE /v1/session-registry/users/{user}/sessions/{session_id}",
    "SessionRegistryService.Close",
    Expect::Routes(&[(
        "DELETE",
        "/v1/session-registry/users/{user}/sessions/{session_id}"
    )])
)]
// corner — a server-streaming READ maps to GET (chunked JSON): the first GET-streaming
// row, the direct contrast to 03d's server-streaming action (Reindex → POST).
#[case::corner_subscribe_server_streaming_read_maps_to_get(
    "AgentSessionService.Subscribe (server-streaming read) -> GET /v1/agent-sessions/events",
    "AgentSessionService.Subscribe",
    Expect::Routes(&[("GET", "/v1/agent-sessions/events")])
)]
// positive — the drive RPC (submit a goal, stream the run) is a side-effecting action
// → POST body:*, even though it too server-streams.
#[case::positive_send_drive_server_streaming_maps_to_post(
    "AgentSessionService.Send (drive) -> POST /v1/agent-sessions/send",
    "AgentSessionService.Send",
    Expect::Routes(&[("POST", "/v1/agent-sessions/send")])
)]
// corner — cancelling a scheduled job is a removal → DELETE by id (a second DELETE-verb
// site outside the CRUD registries).
#[case::corner_scheduler_cancel_maps_to_delete(
    "SchedulerService.Cancel -> DELETE /v1/scheduler/jobs/{id}",
    "SchedulerService.Cancel",
    Expect::Routes(&[("DELETE", "/v1/scheduler/jobs/{id}")])
)]
// corner — reading a job's runs is a child-collection read addressed by the parent id.
#[case::corner_scheduler_history_sub_resource_read(
    "SchedulerService.History -> GET /v1/scheduler/jobs/{id}/runs",
    "SchedulerService.History",
    Expect::Routes(&[("GET", "/v1/scheduler/jobs/{id}/runs")])
)]
// --- 03f: tools / exec / web / forge ----------------------------------------
// corner — running a named tool captures the tool name in the path and carries the
// arguments/context in the body (path param + POST body:*).
#[case::corner_tool_execute_name_in_path_with_body(
    "ToolService.Execute -> POST /v1/tools/{name}/execute",
    "ToolService.Execute",
    Expect::Routes(&[("POST", "/v1/tools/{name}/execute")])
)]
// positive — the sandbox exec (run a command to completion, the largest grant) is an
// action → POST body:*.
#[case::positive_sandbox_exec_action_maps_to_post(
    "SandboxService.Exec -> POST /v1/sandbox/exec",
    "SandboxService.Exec",
    Expect::Routes(&[("POST", "/v1/sandbox/exec")])
)]
// corner — reading a pty's output is a cursor read on a session sub-resource → GET
// /{id}/read (the cursor rides as a query param), NOT a POST.
#[case::corner_pty_read_is_get_on_session_subresource(
    "PtyService.Read -> GET /v1/pty/sessions/{id}/read",
    "PtyService.Read",
    Expect::Routes(&[("GET", "/v1/pty/sessions/{id}/read")])
)]
// corner — a PR is addressed by a NUMERIC id (`uint64 number`); the transcoder binds
// it to the `{number}` capture just like a string id (first numeric path param).
#[case::corner_forge_getpr_numeric_path_param(
    "ForgeService.GetPr -> GET /v1/forge/prs/{number}",
    "ForgeService.GetPr",
    Expect::Routes(&[("GET", "/v1/forge/prs/{number}")])
)]
// corner — clearing the whole todo list is a collection-level DELETE with ZERO path
// params (the /v1/tasks collection also carries POST Write + GET List — three verbs,
// one path, distinct routes).
#[case::corner_task_clear_collection_level_delete(
    "TaskService.Clear -> DELETE /v1/tasks (collection-level delete, no path param)",
    "TaskService.Clear",
    Expect::Routes(&[("DELETE", "/v1/tasks")])
)]
// corner — submitting a review is an outside-world WRITE nested under a numeric parent
// (POST body:* with a `{number}` capture).
#[case::corner_forge_reviewpr_write_under_numeric_parent(
    "ForgeService.ReviewPr -> POST /v1/forge/prs/{number}/reviews",
    "ForgeService.ReviewPr",
    Expect::Routes(&[("POST", "/v1/forge/prs/{number}/reviews")])
)]
// --- 03g1: LLM plane (provider / llm_pool / embed / tokenizer) --------------
// positive — the LAST of the five server-streaming RPCs: streaming completion is a
// side-effecting action → POST body:*. With this the streaming set is complete.
#[case::positive_provider_stream_last_server_streaming(
    "Provider.Stream (server-streaming) -> POST /v1/provider/stream",
    "Provider.Stream",
    Expect::Routes(&[("POST", "/v1/provider/stream")])
)]
// positive — a nested `CompletionRequest` maps to POST body:* (the buffered path).
#[case::positive_provider_complete_nested_maps_to_post(
    "Provider.Complete -> POST /v1/provider/complete",
    "Provider.Complete",
    Expect::Routes(&[("POST", "/v1/provider/complete")])
)]
// corner — a repeated-scalar CONTENT payload (document bodies to embed) maps to POST,
// NOT GET: query strings carry selectors, not payloads (content-payload refinement).
#[case::corner_embed_docs_content_payload_maps_to_post(
    "EmbedService.EmbedDocs -> POST /v1/embed/docs (repeated content payload)",
    "EmbedService.EmbedDocs",
    Expect::Routes(&[("POST", "/v1/embed/docs")])
)]
// corner — the single-query counterpart stays GET: one short query scalar is a
// selector, so it rides as a query param (the direct contrast to EmbedDocs).
#[case::corner_embed_query_single_scalar_maps_to_get(
    "EmbedService.EmbedQuery -> GET /v1/embed/query (single query scalar)",
    "EmbedService.EmbedQuery",
    Expect::Routes(&[("GET", "/v1/embed/query")])
)]
// corner — counting messages carries a repeated NESTED `Message`, so → POST body:*
// (nested-body rule), while the scalar-text `Count` stays GET.
#[case::corner_tokenizer_count_messages_nested_repeated_maps_to_post(
    "TokenizerService.CountMessages -> POST /v1/tokenizer/count-messages",
    "TokenizerService.CountMessages",
    Expect::Routes(&[("POST", "/v1/tokenizer/count-messages")])
)]
// --- 03g2: cognition / memory (memory / context / dimension / mode / digest /
//          reference) -----------------------------------------------------------
// positive — a bounded read whose only field is a scalar `limit` maps to GET (the
// limit rides as a query param), in a fresh proto group.
#[case::positive_episodic_recent_scalar_limit_read_maps_to_get(
    "Episodic.Recent -> GET /v1/episodic/recent (scalar limit as query param)",
    "Episodic.Recent",
    Expect::Routes(&[("GET", "/v1/episodic/recent")])
)]
// corner — a read whose request is a nested `RecallQuery` maps to POST body:*
// (nested-body-read convention) even though it is side-effect-free.
#[case::corner_memory_recall_nested_body_read_maps_to_post(
    "Memory.Recall -> POST /v1/memory/recall (nested RecallQuery in the request)",
    "Memory.Recall",
    Expect::Routes(&[("POST", "/v1/memory/recall")])
)]
// corner — a read keyed by a `safe_segment` slug captures it as a path param (the
// first identifier-is-a-slug capture, contrast the query-param revision reads).
#[case::corner_dimension_recall_slug_path_param(
    "DimensionService.Recall -> GET /v1/dimensions/{dimension} (slug identifier as path param)",
    "DimensionService.Recall",
    Expect::Routes(&[("GET", "/v1/dimensions/{dimension}")])
)]
// corner — a ledger read keyed by `session_id` (path param) whose `keywords_any` is a
// repeated-scalar FILTER stays GET (the keywords ride as repeated query params) — the
// filter half of the content-payload-vs-filter refinement.
#[case::corner_digest_query_repeated_scalar_filter_stays_get(
    "DigestService.Query -> GET /v1/digests/{session_id} (repeated-scalar keyword filter)",
    "DigestService.Query",
    Expect::Routes(&[("GET", "/v1/digests/{session_id}")])
)]
// corner — resolving `@`-mentions carries a whole `prompt` CONTENT payload, so it is
// POST body:* even though it never mutates state (content-payload refinement, scalar
// form: a prompt is a payload, not a URL-friendly selector).
#[case::corner_reference_resolve_content_payload_maps_to_post(
    "ReferenceService.Resolve -> POST /v1/references/resolve (prompt content payload)",
    "ReferenceService.Resolve",
    Expect::Routes(&[("POST", "/v1/references/resolve")])
)]
// boundary — an action whose request is EMPTY still maps to POST body:* (a distiller
// trigger has no query surface); contrast the empty-request GET reads (Preflight).
#[case::boundary_memory_distill_empty_request_action_maps_to_post(
    "Memory.Distill (empty request) -> POST /v1/memory/distill",
    "Memory.Distill",
    Expect::Routes(&[("POST", "/v1/memory/distill")])
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
