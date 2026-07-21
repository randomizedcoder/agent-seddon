//! `LocalSandbox` — today's unconfined `bash -c` spawn. The `local` backend is
//! behaviour-identical to the pre-seam `BashTool`, so selecting it changes
//! nothing; it exists so the other backends are a config swap away.

use crate::run_argv;
use agent_core::{ExecOutput, ExecSpec, Result, Sandbox, SandboxCapabilities};
use async_trait::async_trait;

pub struct LocalSandbox;

#[async_trait]
impl Sandbox for LocalSandbox {
    async fn exec(&self, spec: &ExecSpec) -> Result<ExecOutput> {
        run_argv(&["bash".into(), "-c".into(), spec.command.clone()], spec).await
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            backend: "local".into(),
            available: true, // a plain spawn is always available
            network_off: false,
            private_tmp: false,
            content_addressed: false,
        }
    }
}
