# nix/nixos/agent-stack-vm-test.nix
#
# OPT-IN boot test for `nixosModules.agent-stack` (`nix build .#nixos-agent-stack-vm`).
# Where `nixos-agent-stack-eval` (in the gate) proves the module RESOLVES, this boots
# it in a QEMU VM and proves the units actually START in order: ClickHouse creds →
# ClickHouse (admin hash from the env file) → schema + role passwords, and Postgres
# creds → Postgres → role + password → gateway / sessions / fleet (telemetry writer
# authenticated, stores migrated, roster seeded) → Envoy grpc-web + portal, and Dex
# (mock connector) completing a PKCE code flow for the agent's client over loopback, then
# (a specialisation) the GitHub connector starting with the operator's secret spliced in,
# and the SSO front door (security-hardening 10, S28b): its dev-PKI certificate, the
# gate's check on each route, and a Dex sign-in walked through :8443 (refused for an
# unbound user; a bound one gets a session its RPCs carry).
#
# Not in `nix flake check`: it needs KVM and minutes of VM time (the same reason
# the postgres module only has an eval gate). Run it after touching the module.
#
# The LLM is never dialed — the services only need a resolvable provider config —
# so a dummy endpoint + a dummy key credential (a store path; fine for a test VM,
# never for a real host) stand in.
{
  pkgs,
  module,
}:

pkgs.testers.runNixOSTest {
  name = "nixos-agent-stack-vm";

  nodes.machine =
    {
      pkgs,
      lib,
      config,
      ...
    }:
    {
      imports = [ module ];
      virtualisation = {
        memorySize = 4096;
        cores = 4;
        diskSize = 4096;
      };
      services.agentSeddon = {
        enable = true;
        extraOrigins = [ "http://alias.test:8092" ];
        credentials."llm.token" = pkgs.writeText "dummy-llm-token" "not-a-real-key";
        settings.provider = {
          base_url = "http://127.0.0.1:9/v1";
          model = "dummy";
          api_key_file = "/run/credentials/@unit@/llm.token";
        };
        # Seeded into Postgres. Its token cannot resolve, so the fleet keeps it unadmitted.
        fleet.roster = [
          {
            id = "vm-row";
            user = "acme";
            repo = "acme__widget";
            backend = "github";
            base_url = "";
            token_ref = "env:AGENT_VM_NO_TOKEN";
            skill = "";
            slack_trigger_channel = "";
            slack_progress_channel = "";
            poll_secs = 300;
            enabled = false;
            created_at = 0;
            updated_at = 0;
            forge_id = "";
            transport_id = "";
          }
        ];
      };
      # The host's Grafana with the module's admin password (generated).
      services.grafana = {
        enable = true;
        # A store path, like the dummy LLM key above: fine for a test VM, never a host.
        settings.security.secret_key = "$__file{${pkgs.writeText "grafana-secret-key" "vm-test-only"}}";
      };
      services.agentSeddon.grafana = {
        enable = true;
        generateAdminPassword = true;
      };
      # The operator's own file instead (sops-nix, agenix); the test writes it, then
      # switches to this.
      specialisation.pwfile.configuration.services.agentSeddon.grafana = {
        generateAdminPassword = lib.mkForce false;
        adminPasswordFile = "/run/secrets/grafana-admin";
      };
      # Dex with the mock connector (S26). Its issuer defaults to the front door's /dex;
      # the S26 rows reach Dex on loopback, as the agent's back-channel does.
      services.agentSeddon.dex = {
        enable = true;
        redirectUris = [ "https://dex.test/auth/callback" ];
        mockLogin = true;
      };
      # The front door (S28b), with the dev-PKI certificate. The module sets the
      # gateway's `[auth.gate]`; `[auth]` is the host's, as below.
      services.agentSeddon.frontDoor.enable = true;
      services.agentSeddon.credentials."token-signer.key" =
        # A store path, like the dummy LLM key: fine for a test VM, never a host.
        pkgs.runCommand "vm-token-signer.key" { nativeBuildInputs = [ pkgs.openssl ]; } ''
          openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$out"
        '';
      services.agentSeddon.gateway.settings.auth = {
        mode = "oidc";
        redirect_uris = [ config.services.agentSeddon.frontDoor.callbackUrl ];
        issuers = [ config.services.agentSeddon.dex.issuerSettings ];
        token = {
          issuer = "https://agent.test";
          audience = "agent-seddon";
          signing_key = "/run/credentials/@unit@/token-signer.key";
          session_store = "file";
          session_path = "/var/lib/agent-seddon/auth-sessions";
        };
      };
      # Dex's mock user, bound: the base system leaves it unbound.
      specialisation.signin.configuration.services.agentSeddon.gateway.settings.auth.operator_subjects = [
        "email:kilgore@kilgore.trout"
      ];
      # The GitHub connector instead (#708 draft): Dex starts with the operator's client
      # secret spliced in by `replace-secret`. Nothing dials github.com.
      specialisation.dexgh.configuration.services.agentSeddon.dex = {
        mockLogin = lib.mkForce false;
        github = {
          clientId = "Iv1.vmtest";
          clientSecretFile = "/run/secrets/dex-github";
        };
      };
      environment.systemPackages = [ pkgs.curl ];
    };

  testScript = ''
    machine.start()
    # The base system, whose specialisations the subtests switch to.
    base = machine.succeed("readlink -f /run/current-system").strip()

    with subtest("clickhouse: creds, server, schema"):
        machine.wait_for_unit("agent-seddon-clickhouse-creds.service")
        machine.succeed("test -s /var/lib/agent-seddon/clickhouse/writer.password")
        machine.succeed("grep -q '^AGENT_CH_ADMIN_SHA256=[0-9a-f]\\{64\\}$' /var/lib/agent-seddon/clickhouse/admin.env")
        machine.wait_for_unit("clickhouse.service")
        machine.wait_for_unit("agent-seddon-clickhouse-schema.service")
        # The admin password is the generated one; the server is loopback-only.
        tables = machine.succeed(
            "CLICKHOUSE_PASSWORD=$(cat /var/lib/agent-seddon/clickhouse/admin.password) "
            "clickhouse-client --query 'SHOW TABLES FROM agent'"
        )
        assert "agent_review_drafts" in tables and "agent_events" in tables, tables
        # No password ⇒ rejected (the stock passwordless `default` is gone).
        machine.fail("clickhouse-client --query 'SELECT 1'")

    with subtest("postgres: creds, server, role password"):
        machine.wait_for_unit("agent-seddon-postgres-creds.service")
        machine.succeed("grep -qE '^[0-9a-f]{48}$' /var/lib/agent-seddon/postgres/password")
        machine.succeed("stat -c %a /var/lib/agent-seddon/postgres | grep -qx 700")
        machine.wait_for_unit("postgresql-setup.service")
        # The DSN's password authenticates over TCP (SCRAM), and nothing else does.
        machine.succeed("psql \"$(cat /var/lib/agent-seddon/postgres/dsn)\" -c 'SELECT 1'")
        machine.fail("PGPASSWORD=wrong psql -h 127.0.0.1 -U agent -d agent_config -c 'SELECT 1'")

    with subtest("agents start and listen"):
        for unit in ["gateway", "sessions", "fleet"]:
            machine.wait_for_unit(f"agent-seddon-{unit}.service")
        machine.wait_for_open_port(50100)
        machine.wait_for_open_port(50080)
        machine.wait_for_open_port(50086)
        machine.succeed("stat -c %U /var/lib/agent-seddon/fleet | grep -qx agent-seddon")
        # The generated TOML: the credential path rewritten to this unit, and the repo
        # config's inline placeholder key gone (inline would shadow api_key_file).
        cfg = machine.succeed(
            "cat $(systemctl show -P ExecStart agent-seddon-fleet | grep -o '/nix/store/[^ ]*agent-seddon-fleet.toml')"
        )
        assert 'api_key_file = "/run/credentials/agent-seddon-fleet.service/llm.token"' in cfg, cfg
        assert "\napi_key =" not in cfg, cfg
        # Stores: digests in ClickHouse, the transactional ones in Postgres.
        assert 'dsn_ref = "file:/var/lib/agent-seddon/postgres/dsn"' in cfg, cfg
        assert 'store = "clickhouse"' in cfg and 'store = "postgres"' in cfg, cfg

    with subtest("roster seeded into postgres, idempotently"):
        q = "psql \"$(cat /var/lib/agent-seddon/postgres/dsn)\" -tAc \"SELECT count(*) FROM cards WHERE id = 'vm-row'\""
        machine.wait_until_succeeds(q + " | grep -qx 1")
        machine.succeed("systemctl restart agent-seddon-fleet.service")
        machine.wait_for_open_port(50086)
        machine.succeed(q + " | grep -qx 1")
        machine.succeed("journalctl -u agent-seddon-fleet | grep -q 'fleet roster seeded'")

    with subtest("grpc-web + portal"):
        machine.wait_for_unit("agent-seddon-grpc-web.service")
        machine.wait_for_unit("agent-seddon-portal.service")
        for port in [8090, 8091, 8093, 8092]:
            machine.wait_for_open_port(port)
        machine.succeed("curl -fsS http://127.0.0.1:8092/ | grep -q '<title>Agent Seddon</title>'")
        machine.succeed("curl -fsS -o /dev/null http://127.0.0.1:8092/main.dart.js")
        # CORS: the publicHost origin and an extraOrigins alias are allowed; others are not.
        preflight = (
            "curl -fsS -o /dev/null -D - -X OPTIONS http://127.0.0.1:8093/ "
            "-H 'Access-Control-Request-Method: POST' "
            "-H 'Access-Control-Request-Headers: content-type,x-grpc-web' -H 'Origin: {}'"
        )
        for origin in ["http://127.0.0.1:8092", "http://alias.test:8092"]:
            headers = machine.succeed(preflight.format(origin))
            assert f"access-control-allow-origin: {origin}" in headers.lower(), headers
        headers = machine.succeed(preflight.format("http://evil.test:8092"))
        assert "access-control-allow-origin" not in headers.lower(), headers

    with subtest("grafana: the admin password is generated, applied and rotated"):
        pw_file = "/var/lib/agent-seddon-grafana/admin.password"
        # The password never reaches argv: curl reads `user:password` from a config file.
        user_api = (
            "printf 'user = \"admin:%s\"\\n' \"$({})\" "
            "| curl -fsS -o /dev/null -K - http://127.0.0.1:3000/api/user"
        )
        def applied():
            machine.wait_for_unit("grafana.service")
            machine.wait_for_unit("agent-seddon-grafana-admin.service")
        applied()
        machine.succeed("stat -c %a /var/lib/agent-seddon-grafana | grep -qx 700")
        machine.succeed(f"stat -c %a {pw_file} | grep -qx 600")
        machine.succeed(f"grep -qxE '[A-Za-z0-9]{{20,}}' {pw_file}")
        machine.succeed(user_api.format(f"cat {pw_file}"))
        machine.fail(user_api.format("echo admin"))
        # Rotation as the option describes it: delete the file, restart Grafana.
        machine.succeed(f"cp {pw_file} /root/old.password && rm {pw_file}")
        machine.succeed("systemctl restart grafana.service")
        applied()
        machine.succeed(f"test -s {pw_file} && ! cmp -s {pw_file} /root/old.password")
        machine.succeed(user_api.format(f"cat {pw_file}"))
        machine.fail(user_api.format("cat /root/old.password"))
        # A file ending in a newline (an editor, sops-nix): the password is the line.
        machine.succeed(f"printf 'line-ending-pw-0123\\n' > {pw_file}")
        machine.succeed("systemctl restart grafana.service")
        applied()
        machine.succeed(user_api.format("echo line-ending-pw-0123"))
        journal = "journalctl --no-pager -u agent-seddon-grafana-admin -u grafana.service"
        machine.fail(f"{journal} | grep -qF line-ending-pw-0123")
        # adminPasswordFile: the operator's file, through the same reset.
        machine.succeed(
            "install -d -m 0700 /run/secrets && "
            "(umask 077 && printf operator-pw-4567 > /run/secrets/grafana-admin)"
        )
        machine.succeed(f"{base}/specialisation/pwfile/bin/switch-to-configuration test")
        applied()
        machine.succeed(user_api.format("echo operator-pw-4567"))
        machine.fail(user_api.format("echo line-ending-pw-0123"))
        machine.fail(f"{journal} | grep -qF operator-pw-4567")

    with subtest("dex: the agent's client signs in over loopback"):
        import base64, hashlib, json, urllib.parse
        machine.wait_for_unit("agent-seddon-dex-secret.service")
        machine.succeed("stat -c %a /var/lib/agent-seddon-dex | grep -qx 700")
        machine.succeed("grep -qE '^[0-9a-f]{64}$' /var/lib/agent-seddon-dex/agent-client.secret")
        machine.wait_for_unit("dex.service")
        machine.wait_for_open_port(5556)
        # Loopback only, and the secret's path was replaced by the secret.
        machine.fail("ss -Hltn 'sport = :5556' | grep -v '127.0.0.1:5556'")
        machine.fail("grep -q agent-client.secret /run/dex/config.yaml")
        local, public = "http://127.0.0.1:5556/dex", "https://127.0.0.1:8443/dex"
        doc = json.loads(machine.succeed(f"curl -fsS {local}/.well-known/openid-configuration"))
        assert doc["issuer"] == public, doc
        assert doc["token_endpoint"] == f"{public}/token", doc
        # The agent reads its copy of the secret as a credential.
        machine.succeed(
            "cmp /run/credentials/agent-seddon-gateway.service/dex-client "
            "/var/lib/agent-seddon-dex/agent-client.secret"
        )
        verifier = "v" * 43
        challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).rstrip(b"=").decode()
        cb = "https://dex.test/auth/callback"
        url = f"{local}/auth?" + urllib.parse.urlencode({
            "client_id": "agent-seddon", "redirect_uri": cb, "response_type": "code",
            "scope": "openid email", "state": "st-1", "nonce": "n-1",
            "code_challenge": challenge, "code_challenge_method": "S256",
        })
        # Follow Dex's redirects (auth → mock connector → callback) until it returns to
        # the agent's redirect URI; absolute ones name the public issuer.
        for _ in range(8):
            loc = machine.succeed(f"curl -sS -o /dev/null -w '%{{redirect_url}}' '{url}'").strip()
            assert loc, f"no redirect from {url}"
            if loc.startswith(cb):
                break
            url = loc.replace(public, local, 1) if loc.startswith(public) else urllib.parse.urljoin(url, loc)
        else:
            raise AssertionError(f"Dex never returned to {cb}; last redirect {loc}")
        q = urllib.parse.parse_qs(urllib.parse.urlparse(loc).query)
        assert q["state"] == ["st-1"], loc
        code = q["code"][0]
        reply = json.loads(machine.succeed(
            "curl -fsS -u agent-seddon:$(cat /var/lib/agent-seddon-dex/agent-client.secret) "
            f"-d grant_type=authorization_code -d code={code} -d redirect_uri={cb} "
            f"-d code_verifier={verifier} {local}/token"
        ))
        payload = reply["id_token"].split(".")[1]
        claims = json.loads(base64.urlsafe_b64decode(payload + "=" * (-len(payload) % 4)))
        assert claims["iss"] == public and claims["aud"] == "agent-seddon", claims
        assert claims["nonce"] == "n-1" and claims["email_verified"] is True, claims
        # A wrong secret is refused.
        machine.fail(
            f"curl -fsS -u agent-seddon:wrong -d grant_type=authorization_code -d code={code} "
            f"-d redirect_uri={cb} -d code_verifier={verifier} {local}/token"
        )

    door, ca = "https://127.0.0.1:8443", "/var/lib/agent-seddon-front-door/pki/ca/root.crt"

    def door_get(url, jar="/tmp/jar", extra=""):
        """One request through the front door, redirects not followed: (status, location, headers)."""
        out = machine.succeed(
            f"curl -sS --cacert {ca} -b {jar} -c {jar} -D /tmp/hdr -o /tmp/body "
            f"-w '%{{http_code}} %{{redirect_url}}' {extra} '{url}'"
        ).strip()
        status, _, loc = out.partition(" ")
        return int(status), loc, machine.succeed("cat /tmp/hdr").lower()

    def who_am_i(jar="/tmp/jar", extra=""):
        """AuthService.WhoAmI as grpc-web through the door: (status, body)."""
        status, _, _ = door_get(
            f"{door}/agent.v1.AuthService/WhoAmI", jar,
            "-X POST -H 'content-type: application/grpc-web+proto' -H 'x-grpc-web: 1' "
            f"--data-binary @/tmp/empty-frame {extra}",
        )
        return status, machine.succeed("cat /tmp/body")

    def sign_in(jar="/tmp/jar"):
        """Walk Dex's mock sign-in through the door; the callback's (status, location, headers)."""
        machine.succeed(f"rm -f {jar}")
        status, loc, headers = door_get(f"{door}/auth/login?rd=%2F", jar)
        assert status == 302 and loc.startswith(f"{door}/dex/auth?"), (status, loc)
        assert "cache-control: no-store" in headers, headers
        url = loc
        for _ in range(8):
            status, loc, _ = door_get(url, jar)
            assert status in (302, 303) and loc, (status, url)
            if loc.startswith(f"{door}/auth/callback?"):
                return door_get(loc, jar)
            url = urllib.parse.urljoin(url, loc)
        raise AssertionError(f"Dex never returned to the gate; last redirect {loc}")

    with subtest("front door: certificate, routes and the gate's check"):
        machine.succeed("printf '\\0\\0\\0\\0\\0' > /tmp/empty-frame")
        machine.wait_for_unit("agent-seddon-grpc-web.service")
        machine.wait_for_open_port(8443)
        machine.wait_for_open_port(8444)
        machine.wait_for_open_port(8097)
        # The dev PKI: root-only, the leaf names publicHost, and Envoy holds a copy.
        machine.succeed("stat -c %a /var/lib/agent-seddon-front-door | grep -qx 700")
        machine.succeed(f"stat -c %a {ca.replace('root.crt', 'root.key')} | grep -qx 600")
        machine.succeed("grep -qx 127.0.0.1 /var/lib/agent-seddon-front-door/pki/front-door/sans")
        # Untrusted without the dev root; trusted with it, and HSTS on both listeners.
        machine.fail(f"curl -fsS -o /dev/null {door}/dex/.well-known/openid-configuration")
        for port in [8443, 8444]:
            _, _, headers = door_get(f"https://127.0.0.1:{port}/")
            assert "strict-transport-security: max-age=" in headers, headers
        # /dex is unchecked and names the public issuer; /dexfoo is not Dex.
        status, _, _ = door_get(f"{door}/dex/.well-known/openid-configuration")
        assert status == 200, status
        assert json.loads(machine.succeed("cat /tmp/body"))["issuer"] == f"{door}/dex"
        for path in ["/dexfoo", "/admin", "/agent.v1.AuthService"]:
            status, _, _ = door_get(f"{door}{path}")
            assert status == 403, (path, status)
        # No session: a page goes to sign-in (HyperDX's too, on the main origin); an RPC is 401.
        status, loc, _ = door_get(f"{door}/")
        assert status == 302 and loc == f"{door}/auth/login?rd=/", (status, loc)
        status, loc, _ = door_get("https://127.0.0.1:8444/search")
        # HyperDX's own path is not carried across origins: plain sign-in on the main one.
        assert status == 302 and loc == f"{door}/auth/login", (status, loc)
        status, _ = who_am_i()
        assert status == 401, status
        # A forged bearer is not a credential.
        status, _ = who_am_i(extra="-H 'authorization: Bearer forged.jwt.value'")
        assert status == 401, status

    with subtest("front door: an unbound user's sign-in is refused"):
        status, _, headers = sign_in()
        assert status == 403, status
        assert "__host-seddon=" not in headers, headers
        # Both refusals are audit rows: the unbound sign-in and the unmapped path.
        audit = (
            "CLICKHOUSE_PASSWORD=$(cat /var/lib/agent-seddon/clickhouse/admin.password) "
            "clickhouse-client --query \"SELECT count() FROM agent.agent_auth_events "
            "WHERE event = 'verify_fail' AND reason = '{}'\" | grep -qvx 0"
        )
        for reason in ["unbound", "unmapped_path"]:
            machine.wait_until_succeeds(audit.format(reason), timeout=120)

    with subtest("front door: a bound user signs in, and the RPCs carry the session"):
        machine.succeed(f"{base}/specialisation/signin/bin/switch-to-configuration test")
        machine.wait_for_unit("agent-seddon-gateway.service")
        machine.wait_for_open_port(8097)
        status, loc, headers = sign_in()
        assert status == 302 and loc == f"{door}/", (status, loc, headers)
        assert "httponly" in headers and "secure" in headers and "samesite=lax" in headers, headers
        status, _, _ = door_get(f"{door}/")
        assert status == 200, status
        machine.succeed("grep -q '<title>Agent Seddon</title>' /tmp/body")
        status, body = who_am_i()
        assert status == 200 and "grpc-status:0" in body, (status, body)
        assert "kilgore" in body, body
        # The client's own authorization is overwritten by the gate's, not passed on.
        status, body = who_am_i(extra="-H 'authorization: Bearer forged.jwt.value'")
        assert status == 200 and "grpc-status:0" in body, (status, body)
        # Signing out ends the session: the next page goes back to sign-in.
        status, loc, _ = door_get(f"{door}/auth/logout", extra="-X POST")
        assert status == 302, status
        status, loc, _ = door_get(f"{door}/")
        assert status == 302 and "/auth/login" in loc, (status, loc)

    with subtest("units stay up"):
        machine.sleep(10)
        for unit in ["gateway", "sessions", "fleet", "grpc-web", "portal", "grafana-admin"]:
            machine.require_unit_state(f"agent-seddon-{unit}.service", "active")
        machine.require_unit_state("grafana.service", "active")
        machine.require_unit_state("dex.service", "active")

    with subtest("dex: the GitHub connector gets the operator's secret"):
        gh_secret = "gh-secret-vm-0123"
        machine.succeed(
            "install -d -m 0700 /run/secrets && "
            f"(umask 077 && printf {gh_secret} > /run/secrets/dex-github)"
        )
        # Its own names, so the subtest doesn't lean on the mock-login one's locals.
        local, public = "http://127.0.0.1:5556/dex", "https://127.0.0.1:8443/dex"
        cb = "https://dex.test/auth/callback"
        machine.succeed(f"{base}/specialisation/dexgh/bin/switch-to-configuration test")
        machine.wait_for_unit("dex.service")
        machine.wait_for_open_port(5556)
        # The value, not the path, reached Dex's config.
        machine.succeed(f"grep -qF {gh_secret} /run/dex/config.yaml")
        machine.fail("grep -qF /run/secrets/dex-github /run/dex/config.yaml")
        machine.fail(f"journalctl --no-pager -u dex.service | grep -qF {gh_secret}")
        # Dex loaded the connector: sign-in leads to GitHub with the App's client id.
        url = f"{local}/auth?" + urllib.parse.urlencode({
            "client_id": "agent-seddon", "redirect_uri": cb, "response_type": "code",
            "scope": "openid email", "state": "st-2",
        })
        for _ in range(4):
            loc = machine.succeed(f"curl -sS -o /dev/null -w '%{{redirect_url}}' '{url}'").strip()
            assert loc, f"no redirect from {url}"
            if loc.startswith("https://github.com/"):
                break
            url = loc.replace(public, local, 1) if loc.startswith(public) else urllib.parse.urljoin(url, loc)
        else:
            raise AssertionError(f"Dex never sent the browser to GitHub; last redirect {loc}")
        q = urllib.parse.parse_qs(urllib.parse.urlparse(loc).query)
        assert q["client_id"] == ["Iv1.vmtest"], loc
        assert q["redirect_uri"] == [f"{public}/callback"], loc
  '';
}
