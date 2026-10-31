# nix/nixos/agent-stack-vm-test.nix
#
# OPT-IN boot test for `nixosModules.agent-stack` (`nix build .#nixos-agent-stack-vm`).
# Where `nixos-agent-stack-eval` (in the gate) proves the module RESOLVES, this boots
# it in a QEMU VM and proves the units actually START in order: ClickHouse creds →
# ClickHouse (admin hash from the env file) → schema + role passwords → gateway /
# sessions / fleet (telemetry writer authenticated) → Envoy grpc-web + portal.
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
    { pkgs, ... }:
    {
      imports = [ module ];
      virtualisation = {
        memorySize = 4096;
        cores = 4;
        diskSize = 4096;
      };
      services.agentSeddon = {
        enable = true;
        credentials."llm.token" = pkgs.writeText "dummy-llm-token" "not-a-real-key";
        # A fresh state dir has no `.agent/`: the sqlite digest ledger at its default
        # relative path must create it (the first l2 cutover failed on exactly this).
        settings.digest.store = "sqlite";
        settings.provider = {
          base_url = "http://127.0.0.1:9/v1";
          model = "dummy";
          api_key_file = "/run/credentials/@unit@/llm.token";
        };
        fleet.roster = [ ];
      };
      environment.systemPackages = [ pkgs.curl ];
    };

  testScript = ''
    machine.start()

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

    with subtest("grpc-web + portal"):
        machine.wait_for_unit("agent-seddon-grpc-web.service")
        machine.wait_for_unit("agent-seddon-portal.service")
        for port in [8090, 8091, 8093, 8092]:
            machine.wait_for_open_port(port)
        machine.succeed("curl -fsS http://127.0.0.1:8092/ | grep -q '<title>Agent Seddon</title>'")
        machine.succeed("curl -fsS -o /dev/null http://127.0.0.1:8092/main.dart.js")

    with subtest("units stay up"):
        machine.sleep(10)
        for unit in ["gateway", "sessions", "fleet", "grpc-web", "portal"]:
            machine.require_unit_state(f"agent-seddon-{unit}.service", "active")
  '';
}
