# 02 — Dart / gRPC codegen (buf)

The portal is Dart, the contracts are the same `agent.v1` protos the Rust side
compiles. This part adds **Dart generation** without disturbing the Rust path.

## Two codegens, one source of truth

The `.proto` files stay the single wire source. Rust codegen is unchanged —
`tonic-build`, invoked from
[`agent-proto/build.rs`](../../../crates/agent-proto/build.rs), emitting client +
server stubs and the reflection `FileDescriptorSet`. Dart codegen is **new** and
runs through `buf`.

Note the starting point: **buf today only lints and breaking-checks** — there is no
`buf.gen.yaml` in the repo and buf generates nothing (see [`grpc.md`](../../grpc.md#the-wire-contract--agent-proto)
and `nix/checks/buf.nix`). This is the first *generation* buf drives.

## `buf.gen.yaml`

New file at the repo root, sibling to the existing `buf.yaml`. A **local**
`protoc-gen-dart` plugin (pinned in `nix/`) keeps codegen hermetic — no BSR/network:

```yaml
version: v2
plugins:
  - local: protoc-gen-dart
    out: portal/lib/src/gen
    opt:
      - grpc            # also emit the *.pbgrpc.dart service clients
```

`protoc-gen-dart` emits, per proto:

- `*.pb.dart` — message classes,
- `*.pbenum.dart` / `*.pbjson.dart` — enums + JSON descriptors,
- `*.pbgrpc.dart` — the **service client stubs**.

**The same generated stubs serve both builds.** grpc-web does not change codegen —
only the *channel* differs at runtime (`ClientChannel` native vs
`GrpcWebClientChannel` on the web). So one `buf generate` output drives desktop and
web alike; the fork lives entirely in [`03-flutter-app.md`](03-flutter-app.md)'s
transport abstraction.

## `nix run .#gen-dart`

A new app in `nix/default.nix`, modelled exactly on the existing `buf-image` /
`gen-constants` `writeShellApplication` apps:

```sh
nix run .#gen-dart      # buf generate → portal/lib/src/gen/
```

Like `crates/agent-grpc/src/constants.rs`, the **generated Dart is committed**, so:

- the portal builds without a codegen step on a fresh clone, and
- a later `constants-sync`-style drift check can gate it (regenerate in CI-equivalent,
  fail on diff) — deferred, but the committed-output shape makes it a one-file check
  when wanted.

## Tooling pins

In [`nix/versions.nix`](../../../nix/versions.nix) (the protobuf/gRPC block, next to
the existing `buf`/`grpcurl`/`protoc`):

- `buf` — already pinned,
- `protoc-gen-dart` — **add** (present in `nixos-unstable`).

Add `protoc-gen-dart` to the dev-shell list in
[`nix/packages.nix`](../../../nix/packages.nix) (`allDevPackages`, protobuf/gRPC
block), so `nix develop` has it on `PATH` for `gen-dart`.

Flutter/Dart themselves (the SDK, not this plugin) are pinned in the same file and
covered in [`04-nix-tooling.md`](04-nix-tooling.md).

## Why not tonic-build-style protoc for Dart?

`tonic-build` is Rust-specific. buf is already in the tree, already governs these
protos (lint + breaking), and its plugin model is the idiomatic multi-language
codegen path — so Dart generation is one config file plus one `nix run` app, and it
reuses the module definition (`buf.yaml`'s `crates/agent-proto/proto`) verbatim. No
second protoc invocation to keep in sync.
