//! Task-router end-to-end through the SHIPPED binary over the real HTTP wire
//! (model-router 02 + 02b). `cli_e2e.rs` proves the single-provider path;
//! this proves the ROUTED path: `[agent] provider = "task-router"` +
//! `[[route.upstreams]]` pointing at two loopback `FakeLlm` servers, so the
//! config → builder → factory → `TaskRouter` → openai-compat wire chain runs
//! exactly as a user's does — including which upstream the bytes actually
//! reached, retryable failover between REAL HTTP endpoints, and the strict
//! `[route]` rule parsing failing closed before any request is sent.

mod common;

use common::{status, text, FakeLlm, TempWorkspace};

fn route_config(
    ws: &TempWorkspace,
    kimi_url: &str,
    glm_url: &str,
    rules: &str,
) -> std::path::PathBuf {
    let cfg = ws.path("agent.toml");
    let dir = ws.dir.display();
    std::fs::write(
        &cfg,
        format!(
            r#"
[agent]
provider = "task-router"
context  = "sliding-window"
policy   = "auto-approve"
working_dir = "{dir}"
max_iterations = 4
max_tokens = 256
context_window = 8192
reserve_output = 128
stream = false
system_prompt = "test agent"

# Placeholder (schema quirk): the task-router ignores [provider] but the
# section is required by the loader.
[provider]
base_url = "http://127.0.0.1:1/v1"
model    = "unused"
api_key  = "unused"
max_retries = 0

[route]
[[route.upstreams]]
name = "kimi"
endpoint = "{kimi_url}"
model = "test-model"
tags = ["reasoning"]
tier = "heavy"

[[route.upstreams]]
name = "glm"
endpoint = "{glm_url}"
model = "test-model"
tags = ["reasoning"]
tier = "heavy"

{rules}

[route.default_prefer]
upstreams = ["kimi", "glm"]

[memory]
backend       = "file"
episodic_path = "{dir}/.agent/episodic.jsonl"
semantic_dir  = "{dir}/.agent/memory"

[tools]
enabled = ["ls"]

[search]
auto_index = false

[metrics]
enabled = false

[git]
auto_fetch_secs = 0
"#
        ),
    )
    .expect("write config");
    cfg
}

#[test]
fn positive_routed_turn_reaches_only_the_preferred_upstream() {
    let ws = TempWorkspace::new("route-preferred");
    let kimi = FakeLlm::start(vec![text("from-kimi")]);
    let glm = FakeLlm::start(vec![text("from-glm")]);
    let cfg = route_config(&ws, kimi.base_url(), glm.base_url(), "");

    let (code, stdout, stderr) = common::run_agent(&cfg, &ws, &["say done"]);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.contains("from-kimi"), "got:\n{stdout}");

    // The DECISION is observable at the wire: bytes reached the preferred
    // upstream only — the fallback never saw a request.
    assert!(!kimi.requests().is_empty(), "preferred upstream was dialed");
    assert!(
        glm.requests().is_empty(),
        "fallback must not be dialed on a healthy preferred upstream"
    );
}

#[test]
fn positive_retryable_failure_fails_over_between_real_endpoints() {
    let ws = TempWorkspace::new("route-failover");
    // The preferred upstream answers ONLY retryable 429s (enough copies to
    // absorb any provider-internal retry before the router-level failover).
    let kimi = FakeLlm::start(vec![
        status(429, "slow down"),
        status(429, "slow down"),
        status(429, "slow down"),
        status(429, "slow down"),
    ]);
    let glm = FakeLlm::start(vec![text("from-glm")]);
    let cfg = route_config(&ws, kimi.base_url(), glm.base_url(), "");

    let (code, stdout, stderr) = common::run_agent(&cfg, &ws, &["say done"]);
    assert_eq!(code, 0, "failover must recover the turn; stderr:\n{stderr}");
    assert!(stdout.contains("from-glm"), "got:\n{stdout}");
    assert!(
        !kimi.requests().is_empty(),
        "the preferred upstream was tried first"
    );
    assert!(
        !glm.requests().is_empty(),
        "the fallback served the turn after the 429s"
    );
}

#[test]
fn positive_role_rule_steers_by_config_not_hardcoding() {
    // A rule matching the main loop's stamped role (02b) flips the preference:
    // proof through the shipped binary that the per-call hint reaches the
    // policy (the default would pick kimi; the role rule picks glm).
    let ws = TempWorkspace::new("route-role-rule");
    let kimi = FakeLlm::start(vec![text("from-kimi")]);
    let glm = FakeLlm::start(vec![text("from-glm")]);
    let rules = r#"
[[route.rules]]
match  = { role = "main" }
prefer = { upstreams = ["glm"] }
"#;
    let cfg = route_config(&ws, kimi.base_url(), glm.base_url(), rules);

    let (code, stdout, stderr) = common::run_agent(&cfg, &ws, &["say done"]);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.contains("from-glm"), "got:\n{stdout}");
    assert!(
        kimi.requests().is_empty(),
        "the role rule outranks the default"
    );
}

#[test]
fn adversarial_typoed_rule_fails_closed_before_any_request() {
    // Strict `[route]` match parsing (02b): a typo'd task_mode must abort the
    // BUILD — nonzero exit and, crucially, zero bytes to either upstream.
    let ws = TempWorkspace::new("route-typo");
    let kimi = FakeLlm::start(vec![text("never")]);
    let glm = FakeLlm::start(vec![text("never")]);
    let rules = r#"
[[route.rules]]
match  = { task_mode = "reveiw" }
prefer = { upstreams = ["glm"] }
"#;
    let cfg = route_config(&ws, kimi.base_url(), glm.base_url(), rules);

    let (code, _stdout, stderr) = common::run_agent(&cfg, &ws, &["say done"]);
    assert_ne!(code, 0, "a typo'd match constraint must fail the build");
    assert!(
        stderr.contains("unknown match task_mode"),
        "the error must name the offending constraint, got:\n{stderr}"
    );
    assert!(kimi.requests().is_empty() && glm.requests().is_empty());
}

#[test]
fn positive_model_router_config_textproto_replaces_the_toml_fleet() {
    // The 03 startup loader through the shipped binary: the TOML `[route]`
    // prefers kimi, but `--model-router-config` points at a textproto scenario
    // preferring glm — the flag's fleet must be THE authority (wholesale
    // replace, never a merge), proven by which endpoint the bytes reached.
    let ws = TempWorkspace::new("route-textproto");
    let kimi = FakeLlm::start(vec![text("from-kimi")]);
    let glm = FakeLlm::start(vec![text("from-glm")]);
    let cfg = route_config(&ws, kimi.base_url(), glm.base_url(), "");

    let mrc = ws.path("scenario.textproto");
    std::fs::write(
        &mrc,
        format!(
            r#"
upstreams {{
  id: "kimi"
  kind: "openai-compat"
  enabled: true
  base_url: "{}"
  model: "test-model"
}}
upstreams {{
  id: "glm"
  kind: "openai-compat"
  enabled: true
  base_url: "{}"
  model: "test-model"
}}
policy {{
  default_prefer {{ upstreams: "glm" upstreams: "kimi" }}
}}
"#,
            kimi.base_url(),
            glm.base_url()
        ),
    )
    .expect("write scenario");

    let (code, stdout, stderr) = common::run_agent(
        &cfg,
        &ws,
        &["--model-router-config", mrc.to_str().unwrap(), "say done"],
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.contains("from-glm"), "got:\n{stdout}");
    assert!(
        kimi.requests().is_empty(),
        "the textproto's preference replaced the TOML's"
    );
    assert!(!glm.requests().is_empty());
}

#[test]
fn adversarial_malformed_model_router_config_fails_closed() {
    // A truncated/garbled scenario file must abort the BUILD: nonzero exit, a
    // clear error naming the file, and zero bytes to any upstream.
    let ws = TempWorkspace::new("route-textproto-bad");
    let kimi = FakeLlm::start(vec![text("never")]);
    let glm = FakeLlm::start(vec![text("never")]);
    let cfg = route_config(&ws, kimi.base_url(), glm.base_url(), "");

    let mrc = ws.path("broken.textproto");
    std::fs::write(&mrc, "upstreams { id: \"a\" bogus_field: 1").expect("write");

    let (code, _stdout, stderr) = common::run_agent(
        &cfg,
        &ws,
        &["--model-router-config", mrc.to_str().unwrap(), "say done"],
    );
    assert_ne!(code, 0, "a malformed scenario file must fail the build");
    assert!(
        stderr.contains("model-router config"),
        "the error must name the loader, got:\n{stderr}"
    );
    assert!(kimi.requests().is_empty() && glm.requests().is_empty());
}

#[test]
fn positive_registry_source_seeds_routes_and_persists_the_fleet() {
    // Model-router 04 through the shipped binary: `[route] source = "registry"`
    // seeds the (empty) file store from the TOML fleet at boot, the
    // RegistryRouter routes off the live snapshot — proven at the wire — and
    // the seeded bundle lands on disk as diffable textproto, ready for
    // control-plane edits.
    let ws = TempWorkspace::new("route-registry");
    let kimi = FakeLlm::start(vec![text("from-kimi")]);
    let glm = FakeLlm::start(vec![text("from-glm")]);
    let cfg = route_config(&ws, kimi.base_url(), glm.base_url(), "");
    let mut toml = std::fs::read_to_string(&cfg).unwrap();
    toml = toml.replace(
        "[route]\n",
        &format!(
            "[route]\nsource = \"registry\"\n\n[registry]\nstore = \"file\"\nfile = \"{}\"\nrefresh_secs = 0\n",
            ws.path("fleet.textproto").display()
        ),
    );
    std::fs::write(&cfg, toml).unwrap();

    let (code, stdout, stderr) = common::run_agent(&cfg, &ws, &["say done"]);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.contains("from-kimi"), "got:\n{stdout}");
    assert!(!kimi.requests().is_empty(), "preferred upstream served");
    assert!(glm.requests().is_empty(), "fallback untouched");

    // The seed persisted: the bundle exists, holds both cards, and NO secret.
    let bundle = std::fs::read_to_string(ws.path("fleet.textproto")).expect("seeded bundle");
    assert!(
        bundle.contains("id: \"kimi\"") && bundle.contains("id: \"glm\""),
        "{bundle}"
    );
    assert!(
        !bundle.contains("api_key:"),
        "no raw key field in a card: {bundle}"
    );
}

#[test]
fn positive_registry_source_control_plane_edit_reroutes_without_restart_shape() {
    // The no-restart contract, exercised at the store level the binary uses: a
    // first run seeds the bundle; the "operator" then disables kimi in the
    // bundle (what a control-plane Enable(false) writes); the SECOND run must
    // route to glm purely from the mutated registry — the TOML (which still
    // prefers kimi) no longer decides.
    let ws = TempWorkspace::new("route-registry-edit");
    let kimi = FakeLlm::start(vec![text("from-kimi"), text("from-kimi")]);
    let glm = FakeLlm::start(vec![text("from-glm"), text("from-glm")]);
    let cfg = route_config(&ws, kimi.base_url(), glm.base_url(), "");
    let mut toml = std::fs::read_to_string(&cfg).unwrap();
    toml = toml.replace(
        "[route]\n",
        &format!(
            "[route]\nsource = \"registry\"\n\n[registry]\nstore = \"file\"\nfile = \"{}\"\nrefresh_secs = 0\n",
            ws.path("fleet.textproto").display()
        ),
    );
    std::fs::write(&cfg, toml).unwrap();

    let (code, _out, stderr) = common::run_agent(&cfg, &ws, &["say done"]);
    assert_eq!(code, 0, "seed run; stderr:\n{stderr}");
    assert!(!kimi.requests().is_empty());

    // The control-plane edit: disable kimi in the persisted bundle.
    let bundle = std::fs::read_to_string(ws.path("fleet.textproto")).unwrap();
    std::fs::write(
        ws.path("fleet.textproto"),
        bundle.replacen("enabled: true", "enabled: false", 1),
    )
    .unwrap();

    let (code, stdout, stderr) = common::run_agent(&cfg, &ws, &["say done"]);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.contains("from-glm"), "got:\n{stdout}");
    // The seed did NOT overwrite the edit (idempotence): kimi stays disabled.
    let bundle = std::fs::read_to_string(ws.path("fleet.textproto")).unwrap();
    assert!(bundle.contains("enabled: false"), "{bundle}");
}
