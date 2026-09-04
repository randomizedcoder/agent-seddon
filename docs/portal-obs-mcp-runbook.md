# Runbook: driving the web portal + observability stack (headless l2) with the Playwright MCP

This is the operational memory for bringing the **Flutter web portal** and the
**observability stack** (Grafana / Prometheus / HyperDX) up on a **headless,
podman-only host** (the reference deployment is `l2` = `172.16.50.46`), and for
driving/testing the web UIs with the **Playwright MCP** — including every auth
gotcha that cost time the first time.

If you're re-learning this: read [Gotchas](#gotchas-the-short-list) first — the
non-obvious failures (grpc-web CORS identity headers, the HyperDX `localhost`
cookie domain, Flutter's CanvasKit semantics tree) are all there.

Related: [`observability.md`](observability.md), [`tracing.md`](tracing.md),
[`operating.md`](operating.md), [`grpc.md`](grpc.md),
[`design/portal/`](design/portal/).

---

## 1. What runs where

All services run on l2 and are reached from a browser on another box (`l`) over the
LAN. Nothing needs a browser on l2 — the portal is *served* headless.

| Service | Bind | Started by | Notes |
|---|---|---|---|
| Portal web UI | `0.0.0.0:8092` | `nix run .#portal-web` | `flutter build web` + `static-web-server`; endpoints are `--dart-define` (see §2) |
| Envoy grpc-web | `0.0.0.0:8090→50100`, `0.0.0.0:8091→50080` | `CONTAINER_RUNTIME=podman nix run .#grpc-web-up` | browsers can't speak raw gRPC; two listeners in one container |
| Agent gateway | loopback `:50100` (metrics `:9700`) | `agent --serve-all` | serves every seam |
| Agent sessions | loopback `:50080` (metrics `:9630`) | `agent --serve-sessions` | drives runs from the portal; **needs its own `[search] index_dir`** (§3) |
| Prometheus | `:9090` | native NixOS service (`l2/prometheus.nix`) | scrapes `127.0.0.1:9700` + `:9630` |
| Grafana | `:3000` | native NixOS service (`l2/grafana.nix`) | `admin:admin`; dashboards provisioned from `l2/grafana-dashboards/` |
| HyperDX (ClickStack) | `0.0.0.0:8080`, OTLP `127.0.0.1:4317/4318` | `CONTAINER_RUNTIME=podman CLICKSTACK_UI_HOST=0.0.0.0 nix run .#clickstack-up` | all-in-one; OTLP needs an ingestion key (§6) |
| llama.cpp | `0.0.0.0:8095` | l2 nixos `llama-cpp-mi50.service` | `unsloth/Qwen3-30B-A3B-Instruct-2507-GGUF` |

**Reachability** requires three layers to line up: (1) the NixOS firewall
(`l2/lan-access.nix` opens 8090/8091/8092/8095/3000/9090/8080), (2) each service's
bind address (loopback vs `0.0.0.0`; the grpc-web/clickstack containers publish on
`0.0.0.0`, the gateways stay loopback *behind* Envoy), and (3) the URLs baked into
the portal bundle (§2).

---

## 2. Configurable portal endpoints (`--dart-define`)

`portal/lib/src/config.dart` reads every endpoint from `String/int.fromEnvironment`
with the localhost defaults as fallbacks. The `portal` / `portal-web` nix apps
forward any set `PORTAL_*` env var as `--dart-define`. Keys:

```
PORTAL_GATEWAY_HOST/PORT     PORTAL_SESSIONS_HOST/PORT
PORTAL_GRPC_WEB_URL          PORTAL_SESSIONS_GRPC_WEB_URL
PORTAL_GRAFANA_URL  PORTAL_HYPERDX_URL  PORTAL_PROMETHEUS_URL
PORTAL_WEB_HOST/PORT
```

**For LAN**, set them all to the l2 IP so the *viewer's* browser dials l2, not its
own localhost:

```sh
PORTAL_WEB_HOST=0.0.0.0 \
PORTAL_GRPC_WEB_URL=http://172.16.50.46:8090 \
PORTAL_SESSIONS_GRPC_WEB_URL=http://172.16.50.46:8091 \
PORTAL_GRAFANA_URL=http://172.16.50.46:3000 \
PORTAL_HYPERDX_URL=http://172.16.50.46:8080 \
PORTAL_PROMETHEUS_URL=http://172.16.50.46:9090 \
  nix run .#portal-web
```

> **Web config is build-time.** `fromEnvironment` bakes values at
> `flutter build web`. Changing a URL means a rebuild. After a rebuild, the
> viewer's browser may still serve the **old bundle from Flutter's service worker** —
> hard-reload (Ctrl-Shift-R, twice) or clear site data / use a private window.

---

## 3. Bring the stack up (on l2)

```sh
cd /home/das/Downloads/agent-seddon
nix build .#agent                      # -> ./result/bin/agent

# gateway (serves every seam)
./result/bin/agent --serve-all --config config/agent.toml &

# sessions gateway — MUST use a separate search index_dir, else it dies on the
# tantivy writer lock that --serve-all already holds. Copy the config and point
# [search] index_dir at a scratch dir.
./result/bin/agent --serve-sessions --config /path/to/agent-sessions.toml &

# grpc-web bridge (podman)
CONTAINER_RUNTIME=podman nix run .#grpc-web-up

# portal web (see §2 for the LAN dart-defines)
… nix run .#portal-web

# HyperDX (podman). For LAN login + durable OTLP, set the frontend URL and pin the
# ingestion key (see §6/§7). The key lives in ~/.ssh/hyperdx-credentials.
source ~/.ssh/hyperdx-credentials
CONTAINER_RUNTIME=podman CLICKSTACK_UI_HOST=0.0.0.0 \
  CLICKSTACK_FRONTEND_URL=http://172.16.50.46:8080 \
  CLICKSTACK_INGESTION_API_KEY="$HYPERDX_INGESTION_KEY" \
  nix run .#clickstack-up
```

Grafana + Prometheus are **native NixOS services** — enabled in
`~/nixos/desktop/l2/configuration.nix`, rebuilt with `make` (`sudo nixos-rebuild
switch --flake .`). New nix files must be `git add`ed (flakes ignore untracked).

**Podman notes:** rootless podman needs `~/.config/containers/policy.json`
(`{"default":[{"type":"insecureAcceptAnything"}]}`) and **fully-qualified image
names** (its unqualified-search list is empty) — the nix apps already prefix
`docker.io/`.

---

## 4. The Playwright MCP

Used to drive the web UIs headlessly (navigate, click, type, screenshot) so we can
test the portal and read the obs dashboards without a human at a browser.

### 4.1 Install (l2 `home.nix`)

```nix
home.packages = with pkgs; [ nodejs_24 playwright-driver.browsers ];
home.sessionVariables = {
  PLAYWRIGHT_BROWSERS_PATH = "${pkgs.playwright-driver.browsers}";
  PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS = "true";
};
```

`sessionVariables` apply to **new login shells** — open a fresh shell after the
rebuild. Chromium launches headless on NixOS with **no missing libs** using this
bundle (verified).

### 4.2 Version matching (important)

`@playwright/mcp` (npm) pins a Playwright version that expects a **matching browser
revision**. The nix `playwright-driver.browsers` bundle is a fixed Playwright
version (check: `nix eval --raw nixpkgs#playwright-driver.version` → e.g. `1.61.1`,
which ships chromium-1228). Pin the MCP to the release built on that same
Playwright minor, or its chromium revision won't be in the bundle. As of writing,
`@playwright/mcp@0.0.76` pins Playwright 1.61 (matches); `@latest` pinned 1.63
(does not).

### 4.3 Register (local scope, chromium headless)

```sh
claude mcp add playwright \
  -e PLAYWRIGHT_BROWSERS_PATH="$(nix eval --raw nixpkgs#playwright-driver.browsers)" \
  -e PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS=true \
  -- npx -y @playwright/mcp@0.0.76 --browser chromium --headless
```

> A newly-added MCP server loads at **Claude Code startup**, not mid-session —
> restart Claude Code (or the session) before its tools appear. `claude mcp list`
> showing `✔ Connected` is only a health probe.

### 4.4 Ad-hoc driving without the MCP

For one-off scripts (what the obs setup used), a local `playwright` install works:

```sh
mkdir -p ~/scratch/pw && cd ~/scratch/pw && npm init -y && npm i playwright@1.61.1
export PLAYWRIGHT_BROWSERS_PATH=$(nix eval --raw nixpkgs#playwright-driver.browsers)
export PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS=true PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1
node your-script.mjs
```

---

## 5. Driving the **Flutter** portal (CanvasKit)

The portal renders to a `<canvas>` with CanvasKit — **there is no DOM for buttons
or text**, so selector/role-based automation sees nothing by default. Two things
make it drivable:

1. **Enable Flutter's accessibility semantics tree** (a hidden DOM the a11y layer
   builds). Trigger it once per page load:
   ```js
   await page.evaluate(() => {
     const e = document.querySelector('flt-semantics-placeholder');
     if (e) e.dispatchEvent(new Event('click', { bubbles: true }));
   });
   ```
   After this, `flt-semantics[role=button]`, `[aria-label=…]`, textboxes, etc.
   appear. The obs launcher cards expose real labels ("Grafana…"); **the nav-rail
   icons expose `role=button` with *empty* labels** — click them by **geometry**
   (left rail, 80px column: Launch y≈96, Prompts y≈160, Agent y≈224 at 1400×900).

2. **Text input** works: click the goal field's location, then
   `page.keyboard.type(...)` + `Enter` (the goal bar's `onSubmitted` fires `Send`).

3. **Reading the transcript**: Flutter's `ListView` text isn't fully in the a11y
   tree — **screenshot** to verify run output rather than scraping semantics.

---

## 6. Auth, per service

### Portal ⇄ gateways (grpc-web CORS)

The portal attaches identity metadata `x-agent-user-id` / `x-agent-session-id` on
every `Send`; grpc-web turns those into request headers. **Envoy's CORS
`allow_headers` must list them** or the browser blocks `Send` (preflight fail →
`net::ERR_FAILED`), while metadata-free calls like `SessionRegistry.Open` succeed —
so the portal mints a session but never runs. Fixed in `nix/portal/default.nix`
(both listeners); verify:

```sh
curl -s -i -X OPTIONS http://127.0.0.1:8091/agent.v1.AgentSessionService/Send \
  -H 'Origin: http://l2:8092' -H 'Access-Control-Request-Method: POST' \
  -H 'Access-Control-Request-Headers: x-agent-user-id,x-agent-session-id' \
  | grep -i access-control-allow-headers   # must include x-agent-user-id,x-agent-session-id
```

The Agent view's **"Not receiving events"** is the *idle* state (Subscribe with no
session → `InvalidArgument`), not an error — send a goal to bring it live.

### Grafana

- Login: **`admin` / `admin`** (default) on first use.
- Datasource: Prometheus is provisioned (`l2/grafana.nix`), `http://localhost:9090`.
- **Dashboards** are provisioned from `~/nixos/desktop/l2/grafana-dashboards/` —
  drop a JSON there and rebuild (the provider loads the whole dir). The
  `agent-seddon-overview.json` dashboard (uid `agent-seddon-overview`) covers
  request rate by provider, API-call error rate (`finish_reason`), provider/API
  latency p50/p95/p99, TTFT, iterations, mode switches, and backpressure.
- Push a dashboard live without a rebuild (admin API):
  ```sh
  jq '{dashboard: ., overwrite: true, folderId: 0}' dash.json \
    | curl -s -u admin:admin -H 'Content-Type: application/json' \
        -X POST http://172.16.50.46:3000/api/dashboards/db -d @-
  ```
- **The agent exposes domain metrics, not per-RPC-path gRPC metrics** — "requests
  by path" maps to *by provider/model*; error rate = `agent_api_calls_total`
  where `finish_reason != "stop"`, plus `agent_grpc_overload_shed_total` /
  `agent_pool_saturation_shed_total` for backpressure. Panels that can be empty
  (zero errors) use `… or vector(0)` so they read `0`, not "No data".

### Prometheus

Open (no auth). Confirm scrape health:
`curl -s 'http://172.16.50.46:9090/api/v1/targets?state=active'` → `agent-gateway`
and `agent-sessions` should be `health=up` (unrelated `node`/`mq-cake` jobs may be
down — ignore).

### HyperDX (the fiddly one)

HyperDX needs a **first-run account** and an **ingestion API key**; unauthenticated
OTLP is rejected (connection reset). The clean setup uses two container env vars
(both supported by `clickstack-up`, see §3):

- **`CLICKSTACK_FRONTEND_URL=http://<lan-ip>:8080`** → HyperDX's own app/redirect
  URL. This is what makes **LAN login work**: HyperDX binds its session cookie to
  this host, so with it set to the LAN IP you can log in from another box directly
  (no tunnel). Without it (default `localhost`), login via the LAN IP *appears* to
  succeed (`303`) then bounces to `/login` because the cookie doesn't apply.
- **`CLICKSTACK_INGESTION_API_KEY=<uuid>`** → pins the OTLP ingestion key so it's
  known up front and **survives container recreation**. Set the agent's
  `otlp_headers` to the same value (§7). Without it, HyperDX mints a random key you
  must scrape from the UI, and it changes every time the container is recreated.

1. **Register** at `http://<lan-ip>:8080/register` (once per fresh container — the
   account lives in the container's writable layer, so `clickstack-down` wipes it).
   **Password policy:** 12–72 chars, with upper, lower, a digit, and a special char
   (`!@#$%^&*(),.?":{}|<>;-+=`). Account: `admin@hyperdx.local` / password in
   `~/.ssh/hyperdx-credentials`.
2. **Login from `l`:** open the LAN IP (`http://172.16.50.46:8080`) — the portal's
   "Open HyperDX" link already uses the IP, matching `CLICKSTACK_FRONTEND_URL`.
   (If you browse by hostname `l2:8080` instead, the cookie won't match — use the IP,
   or an SSH tunnel to `localhost:8080`.)

> **Disabling auth:** not possible on this image. `HYPERDX_IMAGE=all-in-one-auth`
> bakes login into the *frontend*, so the runtime `IS_LOCAL_APP_MODE` flag only
> bypasses the backend — the UI still gates you. A true no-auth deployment needs
> HyperDX's separate `all-in-one-noauth` image, which isn't published under the
> `hyperdx-all-in-one` repo tags (only `2`/`latest`, both auth). On a trusted LAN,
> use `CLICKSTACK_FRONTEND_URL` + login, or a tunnel.

> **Scraping the key from the UI** (only if you didn't pin it): Team Settings →
> **API & Agents** → **Ingestion API key** (the *first* key; the second is a
> Personal API access key). Keys are masked — click **copy** and read the clipboard
> (grant `clipboard-read`/`clipboard-write` on the browser context). Driving the UI
> needs the same host as `FRONTEND_URL`; if that's `localhost`, map it to IPv4 with
> `--host-resolver-rules=MAP localhost 127.0.0.1` (localhost resolves to `::1`,
> which the container doesn't publish on).

### Secrets pattern (`~/.ssh`)

Credentials live in `~/.ssh/hyperdx-credentials` (0600), matching l2's existing
`~/.ssh/*.creds` convention. Never commit them; never print the values — read them
in-process (`source ~/.ssh/hyperdx-credentials`) and pass via env.

```
HYPERDX_PASSWORD=…            # you set this (policy-compliant)
HYPERDX_INGESTION_KEY=…       # pulled from the API & Agents tab
```

---

## 7. Wire OTLP tracing into HyperDX

Agent tracing is gated by `[telemetry] otlp_endpoint` (independent of the ClickHouse
`enabled` flag). Set the endpoint, service name, and the **auth header** (format is
comma-separated `key=value`; ClickStack wants `authorization=<ingestion-key>`):

```toml
[telemetry]
otlp_endpoint     = "http://127.0.0.1:4317"
otel_service_name = "agent-sessions"
otlp_headers      = "authorization=<HYPERDX_INGESTION_KEY>"
```

Restart the gateway. Verify (no auth → `ExportError: ConnectionReset`; with the key,
rows land):

```sh
# export errors?
grep -iE 'ExportError|ConnectionReset|BrokenPipe' sessions.log
# rows arriving?
CONTAINER_RUNTIME=podman nix run .#clickstack-client -- \
  -q "SELECT ServiceName, count() FROM default.otel_traces GROUP BY ServiceName"
```

The `<HYPERDX_INGESTION_KEY>` must equal the key HyperDX uses. The robust way is to
**pin it**: launch `clickstack-up` with `CLICKSTACK_INGESTION_API_KEY=$HYPERDX_INGESTION_KEY`
(§3) so both sides read the same value from `~/.ssh/hyperdx-credentials` and it
survives container recreation. To keep the key out of the repo, have the launch
wiring substitute it into the generated config at start (the config field is inline
`otlp_headers`, with no `_file` variant like the LLM `api_key_file`).

> `otlp_endpoint` uses `127.0.0.1:4317` (loopback) because the gateways run on the
> host and ClickStack publishes OTLP only on loopback — that's independent of the
> UI's `CLICKSTACK_FRONTEND_URL` (which is the LAN IP for browser login).

---

## Gotchas — the short list

1. **grpc-web CORS** must allow `x-agent-user-id,x-agent-session-id`, or `Send`
   fails with `net::ERR_FAILED` while `Open` works (portal looks stuck idle).
2. **Flutter is CanvasKit** — enable the semantics tree; click nav icons by
   geometry (empty labels); verify transcript by screenshot.
3. **Portal web is build-time config** + an aggressive **service worker** —
   hard-reload after rebuilds.
4. **Sessions gateway needs its own `[search] index_dir`** (tantivy writer lock).
5. **HyperDX LAN login** needs `CLICKSTACK_FRONTEND_URL=http://<lan-ip>:8080`, or
   the session cookie (bound to `localhost`) makes login bounce back to `/login`.
   Browse by the same host as `FRONTEND_URL` (the IP, matching the portal link).
6. **HyperDX OTLP needs the ingestion key** in `otlp_headers` (else ConnectionReset)
   — **pin** it with `CLICKSTACK_INGESTION_API_KEY` so it survives recreation.
   **No-auth isn't available** on the `all-in-one-auth` image (frontend bakes login);
   it needs the unpublished `all-in-one-noauth` variant.
7. **Playwright MCP ↔ nix browser bundle** versions must match (chromium revision).
8. **Podman**: fully-qualified image names + `~/.config/containers/policy.json`.
9. **Agent metrics are domain metrics**, not per-RPC-path — dashboard by
   provider/model + `finish_reason`, not by HTTP path.
