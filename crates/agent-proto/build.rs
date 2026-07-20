//! Compile the `.proto` wire contracts into Rust (client + server stubs) via
//! `tonic-build`. Requires `protoc` on `PATH` or via the `PROTOC` env var —
//! supplied by the nix dev shell / crane build (see `nix/`).

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protos = [
        "proto/agent/v1/common.proto",
        "proto/agent/v1/provider.proto",
        "proto/agent/v1/tool.proto",
        "proto/agent/v1/memory.proto",
        "proto/agent/v1/context.proto",
        "proto/agent/v1/policy.proto",
        "proto/agent/v1/search.proto",
        "proto/agent/v1/repo.proto",
    ];
    // Re-run only when a proto changes.
    for p in &protos {
        println!("cargo:rerun-if-changed={p}");
    }
    // Emit a serialized FileDescriptorSet alongside the generated code so the crate
    // can serve gRPC reflection (see `FILE_DESCRIPTOR_SET` in lib.rs).
    let descriptor =
        std::path::PathBuf::from(std::env::var("OUT_DIR")?).join("agent_descriptor.bin");
    tonic_build::configure()
        .build_client(true)
        .build_server(true)
        .file_descriptor_set_path(&descriptor)
        .compile_protos(&protos, &["proto"])?;
    Ok(())
}
