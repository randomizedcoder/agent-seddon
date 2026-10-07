# 07 — Secrets

## Rule

Secret material never enters **git** or the **Nix store**:
- the store is world-readable;
- git history is forever;
- a `rendered/` tree that ArgoCD applies is both.

nix-k8s-examples reads its secret files with `builtins.readFile` (the files have to be tracked in git
for a flake to see them) and emits Secret manifests into the store. We don't copy that.

## What is secret, per role

| Secret name | Keys | Source (local file on the deploying host) | Used by |
|---|---|---|---|
| `agent-signing-key` | `signing.pem`, `previous.pem` (optional) | the token signing key (step-ca or `pki-dev`) | gateway `[auth.token]` |
| `agent-oidc` | `client_secret` | the IdP client secret | gateway `[auth.oidc]` issuer profiles |
| `agent-clickhouse` | `writer_password`, `reader_password` | the ClickHouse credential files (S16) | all roles |
| `agent-postgres` | `dsn` | the Postgres DSN file | gateway, fleet (config store, sessions, scheduler) |
| `agent-forge` | `token` | the forge token file (e.g. `~/.fleet-runpod-host-token`) | fleet |
| `agent-llm` | one key per provider | the provider key files | gateway, sessions, fleet |

TLS keys are **not** in this list. cert-manager writes them into Secrets in the cluster
([05](05-identity-and-pki.md)).

## Flow

`nix run .#k8s-secrets -- --target k3s` runs the `agent-k8s-secrets` crate's `k8s-secrets` binary
(a Rust library + a thin binary, matching the rest of the repo; a nix wrapper puts `kubectl` on
PATH). It:
1. reads a local manifest (`~/.config/agent-seddon/k8s-secrets.toml`, mode `0600`, outside the repo)
   that maps each Secret's keys to file paths, with an `allowed_roots` allowlist;
2. fails closed on every source — canonicalizes the path (a missing file errors), refuses one that
   escapes `allowed_roots` via a symlink, refuses a group- or world-readable file (`mode & 0o077`) or
   one larger than a cap, and rejects an unsafe Secret data key;
3. builds each `Secret` in memory and pipes it to `kubectl apply --server-side -f -`, so no temp file
   and no store path is involved;
4. labels them `app.kubernetes.io/part-of=agent-seddon` and
   `agent-seddon.io/managed-by=k8s-secrets`, and gives them **no** ArgoCD tracking annotation, so
   `prune` never deletes them.

`--dry-run` validates the manifest and prints only the Secret names and their keys — never a value,
and never contacting a cluster. Restarting the Deployments that mount a changed Secret
(`kubectl rollout restart`, except where the file-poll reload covers it, [05](05-identity-and-pki.md))
is a follow-up: no Deployment mounts one of these app Secrets yet — that wiring lands with the
`[forge]`/signing-key config, alongside the mount.

**In the pods:**
- Secrets are mounted as files (`defaultMode: 0400`), never as environment variables.
- The agent reads them through `file:` references. S17's confinement rules
  ([08-data-plane-and-secrets](../security-hardening/08-data-plane-and-secrets.md)) apply, with the
  mount root as an allowed secret directory.

**At rest:**
- k3s: `--secrets-encryption` ([02](02-cluster-platform.md)).
- k8s: an `EncryptionConfiguration` on the apiservers.

## Tests (`k8s-secrets` table tests)

- **positive:** builds the right Secret from files.
- **negative:** a missing file names the key.
- **boundary:** a file at the cap is accepted, one over is refused.
- **corner:** an optional key that is absent is skipped.
- **adversarial:**
  - a world-readable source is refused;
  - a symlink to `/etc/shadow`-style paths outside the allowed roots is refused;
  - a key name with `/` or `..` is refused;
  - no secret value appears in stdout, stderr or the exception text.

## Later

- The **external-secrets operator** with a real store, or **sops** with age keys, would make
  secrets GitOps-native.
- Either is compatible: only the source changes. The Secret names and mounts stay as they are.
