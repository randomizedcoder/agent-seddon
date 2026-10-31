# Deployment: the NixOS module (`nixosModules.agent-stack`)

The declarative way to run agent-seddon on a NixOS host. It replaces the
hand-started stack in [deployment-l2.md](deployment-l2.md) (nohup'd agents, podman
Envoy and ClickHouse, a `portal-web` dev server) with systemd units that a
`nixos-rebuild` brings up in order.

## What it runs

| Unit | What | Listens |
|---|---|---|
| `agent-seddon-clickhouse-creds` | oneshot: `clickhouse-creds ensure` (per-role passwords, never rotated) + the admin SHA-256 env file | — |
| `clickhouse` | native `services.clickhouse`; admin = generated password (hash via `@from_env`, never in the store), row-policy settings | 127.0.0.1:9000/8123 |
| `agent-seddon-clickhouse-schema` | oneshot, every boot: `schema.sql` + role `ALTER`s (idempotent) | — |
| `agent-seddon-postgres-creds` | oneshot: the Postgres role password (generated once, never rotated) + the DSN file | — |
| `postgresql` + `postgresql-setup` | `services.agentPostgres` ([nixosModules.agent-postgres](../nix/nixos/agent-postgres.nix)): database `agent_config`, role `agent` (SCRAM, password by credential) | 127.0.0.1:5432 |
| `agent-seddon-gateway` | `agent --serve-all` | 127.0.0.1:50100 |
| `agent-seddon-sessions` | `agent --serve-sessions` | 127.0.0.1:50080 |
| `agent-seddon-fleet` | `agent --serve-fleet` (built with `forge-ghapp`) | 127.0.0.1:50086 |
| `agent-seddon-grpc-web` | Envoy, config rendered at build time from `nix/portal/envoy-spec.nix` | 8090/8091/8093 (+8094 REST, loopback only) |
| `agent-seddon-portal` | `static-web-server` over a hermetic Flutter web bundle (`nix/portal/web-bundle.nix`) | 8092 |

## Where the data lives

| Data | Store |
|---|---|
| Telemetry, logs, review drafts and history, usage | ClickHouse |
| Digest ledger (`[digest] store = "clickhouse"`, over the telemetry writer) | ClickHouse |
| Fleet roster and post leases, scheduler jobs, provider registry, role / forge / GitHub-install / transport cards | Postgres (`[config_store]`, `dsn_ref = "file:/var/lib/agent-seddon/postgres/dsn"`, schema applied on start) |
| Prompt library and `context.d` | the flake source, read-only: they are versioned with the code |
| Review worktrees | `/var/lib/agent-seddon/fleet/workspaces` |

`postgres.enable = false` keeps the roster in a JSON file and leaves the other stores at
their defaults. `clickhouse.enable = false` also turns off telemetry and the digest ledger.

The agents run as a dedicated system user `agent-seddon` with state under
`/var/lib/agent-seddon/<service>` and a hardened systemd sandbox. Envoy and the
portal use `DynamicUser`. Each agent's TOML is generated from Nix settings, and the
prompt library and `context.d` come from the flake source.

## Using it from a host flake

```nix
# host flake.nix
inputs.agent-seddon.url = "github:randomizedcoder/agent-seddon";

# in the nixosSystem modules list:
agent-seddon.nixosModules.agent-stack
{
  services.agentSeddon = {
    enable = true;
    publicHost = "172.16.50.46";   # non-loopback ⇒ Envoy + portal bind 0.0.0.0
    openFirewall = true;           # opens 8092 + 8090/8091/8093 only
    # Secrets: paths read at start via systemd LoadCredential, never copied to the store.
    credentials."llm.token" = "/var/lib/secrets/llm.token";
    settings.provider = {
      base_url = "https://llm.example:8000/v1";
      model = "kimi-k2.6";
      api_key_file = "/run/credentials/@unit@/llm.token";
    };
    fleet.roster = [ /* review_fleet roster rows (the JSON the file store holds) */ ];
  };
}
```

`/run/credentials/@unit@/<name>` is rewritten to each unit's own credentials
directory. Per-service settings (`gateway.settings`, `sessions.settings`,
`fleet.settings`) merge over the shared `settings`. Each service can be switched off
with `<service>.enable = false`, and so can `clickhouse.enable`, `grpcWeb.enable`
and `portal.enable`, `postgres.enable`.

The portal's endpoints are `--dart-define` compile-time constants, so they are
derived from `publicHost`. Changing it rebuilds the bundle, and Envoy's CORS origin
follows it. Envoy allows only exact origins: a browser that opens the portal under
another name (`http://l2:8092` when `publicHost` is the IP) is refused, and every
Fleet/Agent call fails with "potential CORS issue". List those names in
`extraOrigins = [ "http://l2:8092" ];`.

The roster is declarative. With Postgres, `fleet.roster` is upserted into the roster on
every fleet start (`[review_fleet] seed_file`): declared rows win, and rows added from
the portal are kept. Without Postgres the JSON file is reinstalled instead, so a portal
edit lasts only until the next restart. Leave `fleet.roster` null to manage the roster
only at runtime.

Deploying a change means merging to `main`, running `nix flake update agent-seddon`
in the host flake, then `nixos-rebuild switch`.

## Testing

- **`nixos-agent-stack-eval`** (in `nix flake check`) is eval-only. It evaluates the
  module in a loopback config and a LAN config, then asserts the units and their
  flags, the user, the credential wiring, ClickHouse and Postgres ordering, and extra CORS origins (a wildcard, path or junk is refused), and the firewall:
  closed by default, and the LAN config opens only the public ports, never 8094,
  9000, 8123 or 5432.
- **`nix build .#nixos-agent-stack-vm`** is opt-in because it needs KVM. It boots
  the module in a QEMU VM and checks that each step comes up in order:
  - creds, ClickHouse, then the schema; it also checks that a passwordless
    connection is refused;
  - Postgres creds, server and role: the DSN authenticates over TCP and a wrong
    password does not;
  - all three agents are listening, with their stores in Postgres and ClickHouse;
  - the declared roster is seeded into Postgres, and a fleet restart does not
    duplicate it;
  - Envoy and the portal are serving the branded bundle, and Envoy's CORS allows
    the `publicHost` origin and an `extraOrigins` alias but not an unknown origin.

  Run it after touching the module.
