//! `NixSandbox` — the headline backend. Runs each command inside the repo's
//! pinned, hermetic flake dev-shell closure (`nix develop <flake> -c bash -c
//! <cmd>`), so the tool environment (toolchain, `$PATH`) is exactly
//! `nix/versions.nix` — reproducible + content-addressed + re-derivable from the
//! lockfile, where the peers use mutable images.
//!
//! This is the **dev-shell mode**. The stronger **sandboxed-derivation mode**
//! (network-off, private `/tmp`, mount confinement — Nix's own build sandbox) is
//! a documented follow-up; `capabilities().network_off` is `false` here so callers
//! don't over-rely on it.

use crate::{on_path, run_argv};
use agent_core::{Error, ExecOutput, ExecSpec, Result, Sandbox, SandboxCapabilities};
use async_trait::async_trait;

pub struct NixSandbox {
    /// The flake directory whose dev-shell supplies the closure (usually the repo
    /// root). The command still runs in `spec.cwd`; only the toolchain comes here.
    flake: String,
}

impl NixSandbox {
    pub fn new(flake: impl Into<String>) -> Self {
        Self {
            flake: flake.into(),
        }
    }
}

#[async_trait]
impl Sandbox for NixSandbox {
    async fn exec(&self, spec: &ExecSpec) -> Result<ExecOutput> {
        if !on_path("nix") {
            return Err(Error::Sandbox(
                "backend `nix` unavailable (no `nix` on PATH)".into(),
            ));
        }
        // `nix develop <flake> -c bash -c <command>`: the toolchain/$PATH is the
        // pinned closure; the command runs in `spec.cwd` (set by `run_argv`).
        run_argv(
            &[
                "nix".into(),
                "develop".into(),
                self.flake.clone(),
                "-c".into(),
                "bash".into(),
                "-c".into(),
                spec.command.clone(),
            ],
            spec,
        )
        .await
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            backend: "nix".into(),
            available: on_path("nix"),
            // Dev-shell mode can't enforce network-off (that's the derivation-mode
            // follow-up); the closure is still content-addressed.
            network_off: false,
            private_tmp: false,
            content_addressed: true,
        }
    }
}
