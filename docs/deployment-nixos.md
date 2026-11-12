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
| `podman-agent-seddon-{mongo,hyperdx-app,hyperdx-otel}` | opt-in `hyperdx.enable`: HyperDX's three containers (`nix/hyperdx`), host network, over the managed ClickHouse | UI 8080; OTLP, API, OpAMP and Mongo on loopback |
| `agent-seddon-hyperdx-env` | oneshot: renders the containers' secret env files (viewer connection, collector's admin password) into `/var/lib/agent-seddon/hyperdx` (0700) | — |
| `agent-seddon-hyperdx-key` (timer, 2 min; not with `hyperdx.auth = false`) | agent user: copies the ingestion key HyperDX mints at sign-up to `hyperdx/otlp.headers` when it changes | — |
| `agent-seddon-hyperdx-restart` (path unit on that file) | root: `systemctl try-restart` of the enabled agents, nothing else | — |
| `agent-seddon-hyperdx-dashboards` | `hyperdx.dashboards` (default: on when `auth = false`): agent user, oneshot after each app start and template change; `hyperdx-provision` writes the agent-seddon dashboards and saved searches through the app's API | — |

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

The module sets `[review] backend = "local"` with `[review_fleet] root`, so each roster
row gets its own mirror and worktree under `fleet/workspaces/<user>/<id>/`. A fleet config
with a root but no local review backend is refused at start: it would review the working
directory's repo instead of the row's.

### HyperDX

`hyperdx.enable = true` runs HyperDX (`nix/hyperdx`, the same images and seeded sources
as `nix run .#hyperdx-up`) as `virtualisation.oci-containers` units. The collector
writes `otel_*` into the managed ClickHouse's `default` database as the admin; the app
reads it as `agent_viewer`. Both passwords reach the containers as env files the
`agent-seddon-hyperdx-env` oneshot writes, never through the store.

Set `hyperdx.frontendUrl` to the URL you browse with (`"http://l2:8080"`): HyperDX's
session cookie binds to that host, and the portal's Launch card links to it. With
`openFirewall` and a LAN `publicHost`, only the UI port (8080) opens. The containers
share the host's network and only Mongo binds loopback: the collector's OTLP ports
(4317/4318) and the app's API and OpAMP ports (8000/4320) listen on every address, so
the firewall is what keeps them off the LAN. Evaluating with the firewall off and a
LAN `publicHost` warns.

The collector opens no OTLP port until the first account registers: the app hands it
its receiver config over OpAMP, and there is none without a team. So after the first
deploy, open `frontendUrl` and sign up; 4317/4318 open a few seconds later. HyperDX
mints the team's ingestion key at sign-up. Within two minutes
`agent-seddon-hyperdx-key` (the agent user, sandboxed like the agents) reads it from
Mongo over loopback and writes `authorization=<key>` to
`/var/lib/agent-seddon/hyperdx/otlp.headers` (0600). A path unit sees the new file, and
`agent-seddon-hyperdx-restart` (root, `systemctl try-restart` only) restarts the
agents. Each agent exports to `127.0.0.1:4317` with `[telemetry] otlp_headers_file`
pointing there and `otel_service_name` set to its unit name. Until the file exists,
the agents log one warning and run without the exporter.

The collector checks that key only when the team's collector authentication is on
(HyperDX → Team settings; off by default). The agents always send it, so turning it
on needs no other change, and it is what keeps other hosts' OTLP out when the
collector's ports are reachable.

**No login (`hyperdx.auth = false`).** For a trusted lab network, the module can run
HyperDX's local app mode instead (`IS_LOCAL_APP_MODE` on the app container). The
image's `/etc/local/entry.sh` overwrites that variable with `REQUIRED_AUTH`, so the
module starts the app from a copy of `entry.sh` without that one line, and the
container refuses to start if the line is not there exactly (a new image that sets it
differently fails loudly instead of quietly keeping the login). There is
no sign-up and no login, and the app serves one built-in team whose ClickHouse
connection and sources are seeded from `DEFAULT_CONNECTIONS` / `DEFAULT_SOURCES` at
start (only when that team has none, so edits made in the UI survive restarts). That
team never enforces collector authentication, so the collector accepts OTLP without a
key, and the module drops the key timer, the key unit and the restart-on-key path; the
agents export with no `otlp_headers_file`. Anyone who can reach `frontendUrl` has full
HyperDX access and anyone who can reach 4317/4318 can write traces, so keep the
firewall on; with `openFirewall` and a LAN `publicHost` the module warns that the UI
is open without a login. Sources, saved searches and dashboards created under a signed-up team stay
in MongoDB but are not shown in local mode (turning `auth` back on shows them again).

**Dashboards (`hyperdx.dashboards`).** With no login, the module also puts four
dashboards in HyperDX — *agent-seddon · Overview* (spans by service, error spans,
slowest operations, gRPC methods), *Model calls* (calls by model, latency, tokens,
finish reasons), *Tools* (calls, p95 latency and errors by tool, policy decisions) and
*Review fleet* (reviews by repo and outcome, review duration, collectors, sandboxed
commands; the first two count only reviews that ran, `outcome` `drafted` or `failed`) — and
four saved searches (error spans, model calls, tool calls over 10 s, fleet reviews). They
are data in `nix/hyperdx/dashboards.nix`; each names its source by kind (`"$trace"`),
because HyperDX gives the seeded sources random ids. `hyperdx-provision`
(`crates/agent-hyperdx`) reads the team's sources from the API, swaps in the real ids
and creates or updates each item by name, tagged `agent-seddon`, so HyperDX's own schema
checks every write. It runs after each app start (a wiped Mongo reseeds the sources) and
when the template changes, retrying until the API and the sources are up. An item you
edit in the UI is overwritten on the next run (copy it under another name to keep
changes); a same-named one without the tag is left alone. The image's own dashboard
provisioner is not used: it looks the team up in Mongo, and the local-mode team has no
`teams` document. Needs `auth = false` (with a login the API wants a session). The
`hyperdx-dashboards` check runs `hyperdx-provision --check` over the template in the
gate (placeholders, names, tags, grid layout); the tiles were also checked against the
image's own zod schema (`hyperdx/hyperdx:2`, 2026-11-07).

Model calls and tool calls carry the OpenTelemetry GenAI attributes
(`gen_ai.operation.name`, `gen_ai.request.model`, `gen_ai.usage.*`,
`gen_ai.tool.name`, `gen_ai.conversation.id`), so HyperDX's **Dashboards → LLM** page
shows calls, tokens, cost, latency and errors per model and session. To browse every
span instead, open **Search** and pick the **Traces** source. Verified on l2
(2026-11-04): sign-up → key copied → agents restarted in the same second → spans from
all three agents in `default.otel_traces`.

The portal's Grafana/Prometheus/HyperDX links are compile-time defines, but a loopback
host in one (the defaults are `localhost`) is replaced by the host the page was opened
under, so `http://l2:8092` links to `http://l2:9090`. Without `hyperdx.enable` the
HyperDX card is hidden.

### Grafana dashboards

`grafana.enable = true` adds the agent-seddon dashboards to the host's own
`services.grafana`, in a folder named "agent-seddon":

| uid | shows |
|---|---|
| `agent-seddon` | the overview: active runs, runs, tokens, reviews, tool and provider errors |
| `agent-seddon-llm` | model calls, TTFT, tokens, prompt-cache hits, spend, the task router, gen_ai spans |
| `agent-seddon-review` | the latest drafts, triggers, repo indexes, collectors, findings, tool status |
| `agent-seddon-tools` | tool calls, errors and latency, the sandbox, web fetches, references |
| `agent-seddon-cognition` | context size, compaction, memory, task modes, the gate, the distiller |
| `agent-seddon-platform` | gRPC, the config store, policy and auth, transport, search, the repo backend |
| `agent-seddon-traces` | spans, error spans and gen_ai conversations from `default.otel_traces` |
| `portal-gui-perf` | the portal GUI perf runs |

The dashboards are generated from `nix/grafana/dashboards.nix` into the store and
can't be edited in place; save a copy to change one. Each dashboard picks its
Prometheus and ClickHouse data sources with a variable, so it uses the Prometheus data
source the host already has. Set `grafana.prometheusUrl` to provision one (uid
`prometheus`) on a host without one; behind the front door that is
`http://127.0.0.1:9090/prometheus`, since Prometheus then serves under `/prometheus/`. The module provisions a ClickHouse data source
(uid `clickhouse`) that logs in as the read-only `agent_viewer`. Grafana reads the
viewer password from `$__file{/run/credentials/grafana.service/ch-viewer}`, which
systemd `LoadCredential` copies from `/var/lib/agent-seddon/clickhouse/viewer.password`.
The module links the ClickHouse plugin into Grafana's plugins directory, which it makes
grafana's own (systemd-tmpfiles refuses a link under a root-owned directory inside
grafana's). It does not use `declarativePlugins`, which turns off plugin preinstall.
Grafana 13.3 and later no longer bundles Prometheus, and preinstall downloads it from
grafana.com, which an offline host can't reach; the module links nixpkgs'
`grafanaPlugins.prometheus` too, keeping a copy preinstall already fetched. A host that
sets `declarativePlugins` itself must add `grafanaPlugins.grafana-clickhouse-datasource`
(and, on 13.3+, `grafanaPlugins.prometheus`) to that list.

#### The admin password

Grafana's `admin` password is set once, when Grafana creates its database, and
nixpkgs leaves it at a public default. Either option replaces it:

| option | the password |
|---|---|
| `grafana.adminPasswordFile = "/run/secrets/grafana-admin"` | read from that file (a path outside the store: sops-nix, agenix or a file you put there) |
| `grafana.generateAdminPassword = true` | random, made once by `agent-seddon-grafana-admin-password` in `/var/lib/agent-seddon-grafana/admin.password` (root only) |

Read the generated one with `sudo cat /var/lib/agent-seddon-grafana/admin.password`;
for a new one, delete that file and `systemctl restart grafana`. The password is the
file's first line, so a trailing newline is fine.
Grafana reads it as `$__file{/run/credentials/grafana.service/admin-password}`. Since
an existing database keeps its old password, `agent-seddon-grafana-admin` runs after
every Grafana start: it waits for `/api/health`, then runs `grafana cli admin
reset-admin-password --password-from-stdin`. The password never lands on a command
line or in the journal. Both options need `grafana.enable` and Grafana's default
sqlite database (no `database.url` DSN) with `server.protocol` http or https, and
setting both is an error. The generated file is required: if the generator fails,
Grafana does not start. The reset is not: if it keeps failing, Grafana stays up with
the old password, so check `systemctl status agent-seddon-grafana-admin` after a change.
Either one sets `services.grafana.settings.security.admin_password`, so remove any
value the host sets there.

#### Checking every panel

`sudo agent-seddon-obs-check` (installed with `grafana.enable`) runs every query of the
agent-seddon dashboards through Grafana's own query API (`/api/ds/query`), as `admin`
over loopback, with the dashboard variables filled in the way the browser fills them
and Grafana's macros left to Grafana. It prints each failed query with Grafana's
message and a summary, and exits 1 if any failed. An empty result is not a failure (a
quiet host has none); `--verbose` lists them. `--from now-24h` widens the window (the
default is the last 6 hours). Without `adminPasswordFile` or `generateAdminPassword`,
pass `--password-file` yourself. The password is read from the file and only ever sent
as basic auth. The trace panels read `default.otel_traces`, which HyperDX's collector
creates. Without `hyperdx.enable` the wrapper passes `--skip-table` for the collector's
tables, so those queries are counted as skipped instead of sent (`--verbose` names each
one). Only a query whose SQL names the table outright is skipped; any other query is
still sent. The VM test runs it against Prometheus behind the door, and the
`grafana-queries` check runs `obs-check --check` in the gate: every query's variables
fill in and every data source it names is one the module provisions.

### Dex (sign-in with GitHub)

`dex.enable = true` runs Dex on `127.0.0.1:${dex.port}` (default 5556) as the agent's
login provider, the first piece of the front door in
[security-hardening 10](design/security-hardening/10-sso-front-door.md):

```nix
services.agentSeddon.dex = {
  enable = true;
  issuer = "https://172.16.50.46:8443/dex";            # the front door's /dex; no trailing slash
  redirectUris = [ "https://172.16.50.46:8443/auth/callback" ];
  github = {
    clientId = "Iv1.…";                                 # callback <issuer>/callback
    clientSecretFile = "/etc/agent-seddon/secrets/dex-github";
    orgs = [ "my-org" ];                                # optional
  };
};
```

| unit | does |
|---|---|
| `agent-seddon-dex-secret` | once, as root: a random client secret for the agent in `/var/lib/agent-seddon-dex/agent-client.secret` (0700 directory) |
| `dex` | the nixpkgs Dex, sqlite in `/var/lib/dex`, the agent registered as client `agent-seddon`; both secrets are put into `/run/dex/config.yaml` at start by root, never into the store |

The agents get the client secret as the credential `dex-client`. The module does not
turn the agent's `[auth]` on: until the front door lands, the portal sends no token.
`dex.issuerSettings` is the ready `[[auth.issuers]]` entry: the public issuer,
and `jwks_url` / `backchannel_url` on loopback, so the agent never goes through the
front door to reach Dex. A host that turns auth on merges it into
`settings.auth.issuers`. Dex tokens carry no tenant, so every sign-in maps to
`dex.tenant` (default `local`). `dex.mockLogin` swaps GitHub for Dex's
`mockCallback` connector, for tests.

A loopback `dex.issuer` (`http://127.0.0.1…`, for a test) must use `dex.port`: the
back-channel is built from the port, so any other one is refused at eval.

Two things about the `issuerSettings` entry:

- **It must go in `[[auth.issuers]]`.** The single-issuer `[auth]` form has no
  `backchannel_url`.
- **It sets `require_email_verified = true`.** Dex's GitHub connector can report an
  email GitHub hasn't verified, and that person is refused. To admit them anyway,
  override the field when merging the entry into `settings.auth.issuers`; binding
  (F2) then matches an email nobody proved they own.

### The front door (single sign-in)

`frontDoor.enable = true` serves `https://<publicHost>:8443` (the portal, its RPCs,
`/dex`, `/grafana`, `/prometheus`) and `:8444` (HyperDX) from the same Envoy, and the
gateway's `[auth.gate]` checks every request
([security-hardening 10](design/security-hardening/10-sso-front-door.md)). A page with no
session goes to `/auth/login`, an RPC gets a 401, and a signed-in browser's RPCs carry
its agent token (the client's own `authorization` is overwritten).

The module sets `[auth.gate]` on the gateway (listen `127.0.0.1:8097`, `frontDoor.issuer`,
main origin `frontDoor.origin`), Dex's issuer to `<origin>/dex`, and adds the gate's
callback to Dex's redirect URIs. Turning `[auth]` on stays the host's step, and the
build refuses a front door until it is done. It goes in the host-wide `settings.auth`,
not `gateway.settings.auth`: the door forwards the token to sessions and the fleet too,
and removes the client's `x-agent-user-id`, so each one names the tenant by verifying
the token itself. The build refuses an agent behind the door without `mode = "oidc"`.

```nix
services.agentSeddon = {
  frontDoor.enable = true;
  dex = { enable = true; github = { … }; };
  settings.auth = {
    mode = "oidc";
    redirect_uris = [ config.services.agentSeddon.frontDoor.callbackUrl ];
    issuers = [ config.services.agentSeddon.dex.issuerSettings ];
    operator_subjects = [ "email:you@example.org" ];   # or role bindings
    token = {
      issuer = "https://172.16.50.46:8443";
      audience = "agent-seddon";
      signing_key = "/run/credentials/@unit@/token-signer.key";  # credentials."token-signer.key"
      session_store = "postgres";
    };
  };
};
```

The certificate: with `frontDoor.certFile` / `keyFile` unset, the
`agent-seddon-front-door-pki` unit mints a leaf for `publicHost` from the dev PKI
(`nix run .#pki-dev`'s tool) in `/var/lib/agent-seddon-front-door` (root, 0700). A
daily timer re-mints it under the same root when it has 30 days left or `publicHost`
changed, and restarts Envoy to load it. Trust `/var/lib/agent-seddon-front-door/pki/ca/root.crt`
in the browser once. A real certificate is the two file options. Envoy (a
`DynamicUser`) reads either pair as credentials, so the key stays root-only on disk.

`publicHost` is checked the way the certificate tool checks a SAN: an IPv4 address or a
lowercase host name. `frontDoor.extraOrigins` are `https://host[:port]` with the same host
rule and a port of 1-65535. So a bad value fails the build, not the certificate unit at
boot.

**The tools behind the door** (F6):

- **Grafana** (the host's `services.grafana`): the module turns on `auth.proxy`. A signed-in
  user is signed in to Grafana as their front-door subject (`X-Seddon-User`, created on first
  visit), with the role the gate maps: operator → Admin, `obs_editor` → Editor, otherwise
  Viewer (`X-Seddon-Role`, synced on each request). Envoy replaces both headers on `/grafana`,
  and Grafana takes them only from 127.0.0.1. The login form is off, and Grafana serves from
  `<origin>/grafana/` (`root_url`, `serve_from_sub_path`). Basic auth stays on, so the `admin`
  account with `grafana.adminPasswordFile` / `generateAdminPassword` is the break-glass login.
  Envoy drops `authorization` on `/grafana`, so it works on loopback only, and the build
  needs Grafana to listen there (`http_addr = "127.0.0.1"`, nixpkgs' default). It also needs
  `http_port` 3000 over `protocol = "http"`, which is where the door sends it.
- **HyperDX**: the door is its sign-in, so `hyperdx.auth` defaults to (and must be) `false`,
  and `hyperdx.frontendUrl` defaults to (and must be) `https://<publicHost>:8444`. The UI's
  own port (8080) has no login then, so `openFirewall` no longer opens it: 8444 is the way in.
- **Prometheus** stays the host's, on loopback (`listenAddress = "127.0.0.1"`; the build
  refuses another address with the door on). The door sends `/prometheus/` to `127.0.0.1:9090`, and a
  build warning names what the host sets:
  `services.prometheus.webExternalUrl = "https://<publicHost>:8443/prometheus/"`. From then on
  Prometheus serves under `/prometheus/`, so local clients (a Grafana data source,
  `grafana.prometheusUrl`) use `http://127.0.0.1:9090/prometheus`.

**Everything else on loopback** (F9). With the door on, only the door (8443, 8444) and
the GitHub webhook listener (8096) bind `publicHost`'s address; the portal server and the
grpc-web listeners (8090-8093) move to 127.0.0.1, and the portal calls its RPCs at the
door's own origin. `openFirewall` on a LAN `publicHost` opens 8443 and 8444 and nothing
else (the webhook is still the host's to open, if it takes deliveries).

HyperDX runs with `--network=host` and binds every address, so the firewall is what keeps
it (and anything else) behind the door. The build refuses the door when:

- `networking.firewall.enable = false`;
- `networking.firewall.trustedInterfaces` has anything but `lo` (it opens every port on
  that interface; open what the interface needs with
  `networking.firewall.interfaces.<name>.allowedTCPPorts` instead);
- the firewall opens a tool port, in `allowedTCPPorts` or inside an `allowedTCPPortRanges`
  range, globally or on any interface but `lo`. The tool ports are the portal's, grpc-web's
  and REST's, the agents', the gate's, Dex's, Grafana's, Prometheus's, HyperDX's (UI, API,
  OpAMP, OTLP, Mongo), ClickHouse's and Postgres's. The message lists each one it found and
  where.

The checks read the NixOS firewall options only: rules from `extraCommands`,
`extraInputRules` or a hand-written nftables ruleset are invisible to them, so review
those by hand. The REST transcoder (8094) stays loopback-only, as before: reach it over
`ssh -L 8094:127.0.0.1:8094 <host>`.

**The portal behind the door** (S31). The bundle is built with `PORTAL_AUTH=door`: the
door's cookie signs it in, so the page holds no token. It asks `WhoAmI` once. A 401 sends
the browser to `/auth/login?rd=<the page's path>`, and signing out POSTs `/auth/logout`.
The door adds the bearer and the session to each call. Its Launch cards link to the door's
`/grafana/` and `/prometheus/` (when the host runs them) and to HyperDX on `:8444`. A
`portal.defines` entry still overrides a link, but the build refuses any `PORTAL_AUTH`
other than `door` while the door is on.

Deploying a change means merging to `main`, running `nix flake update agent-seddon`
in the host flake, then `nixos-rebuild switch`.

## Testing

- **`nixos-agent-stack-eval`** (in `nix flake check`) is eval-only. It evaluates the
  module in a loopback config and a LAN config, then asserts the units and their
  flags, the user, the credential wiring, ClickHouse and Postgres ordering, and extra CORS origins (a wildcard, path or junk is refused), and the firewall:
  closed by default, and the LAN config opens only the public ports, never 8094,
  9000, 8123 or 5432. A HyperDX config checks the three containers, Mongo bound to
  loopback, passwords only in env files, the unit ordering, the key timer, the agents'
  OTLP settings and that only 8080 opens (never 4317/4318/8000/4320/27017). A Grafana
  config checks the dashboards provider, the ClickHouse source as `agent_viewer` with a
  `$__file` credential, the `LoadCredential` and ordering, the plugin link (with `declarativePlugins` unset), and that a
  Prometheus source appears only when asked for. A bad
  `frontendUrl` or HyperDX without ClickHouse is refused. A front-door config checks
  `[auth.gate]` on the gateway only (on Envoy's gate port), Dex's issuer and callback,
  the dev-PKI unit, timer and Envoy credentials (or an operator certificate's), and
  8443 / 8444 in the firewall; it refuses a missing grpc-web or gateway, a lone or
  unsafe cert/key path, an http or wildcard origin, an unusable `publicHost` (including
  what pki-dev refuses: an empty label, an over-long label, a numeric TLD, upper case), and
  a gateway `[auth]` without oidc, the callback, or the issuer. With the host's Grafana it
  checks `auth.proxy`, the sub-path and the login form off, and refuses Grafana off loopback,
  off port 3000 or on a socket. HyperDX behind the door needs `auth = false` (the default
  there) and the door's origin, and its 8080 stays closed while 8444 opens; and
  the host's Prometheus gets a warning until `webExternalUrl` is the door's `/prometheus/`.
  The lockdown rows: with the door only 8443 / 8444 open and the portal binds loopback, and
  the build refuses the firewall off, a trusted interface other than `lo`, Prometheus off
  loopback, and each tool port opened globally, in a range or on an interface; ssh, the
  webhook, the door's own ports, ranges next to a tool port and `lo` are accepted.
- **`nix build .#nixos-agent-stack-vm`** is opt-in because it needs KVM. Its last row (S30)
  switches to a LAN specialisation and probes it from a second VM: the door answers (a page
  with no session goes to sign-in), the portal and grpc-web are bound to loopback, and every
  other port anything listens on is closed from outside. It boots
  the module in a QEMU VM and checks that each step comes up in order:
  - creds, ClickHouse, then the schema; it also checks that a passwordless
    connection is refused;
  - Postgres creds, server and role: the DSN authenticates over TCP and a wrong
    password does not;
  - all three agents are listening, with their stores in Postgres and ClickHouse;
  - the declared roster is seeded into Postgres, and a fleet restart does not
    duplicate it;
  - Envoy and the portal are serving the branded bundle, and Envoy's CORS allows
    the `publicHost` origin and an `extraOrigins` alias but not an unknown origin;
  - the front door: the dev-PKI leaf (root-only directory, the SAN record) verifies
    against the root and not without it; both listeners send HSTS; `/dex` is unchecked
    and names the public issuer; `/dexfoo`, `/admin` and a malformed RPC path get 403;
    with no session a page goes to sign-in (Grafana's and HyperDX's too) and an RPC (forged bearer
    or none) gets 401;
  - Dex's mock sign-in walked through `:8443`: refused for an unbound user (no
    session cookie; `unbound` and `unmapped_path` audit rows in ClickHouse), and, once
    a specialisation binds the user, a `Secure; HttpOnly; SameSite=Lax` session whose
    `WhoAmI` succeeds through the door even with a forged `authorization`. The same session
    reaches `/grafana/` as that user with the Admin role (an operator), and a forged
    `X-Seddon-User` is replaced. Logout then sends the next page back to sign-in.

  Run it after touching the module.
