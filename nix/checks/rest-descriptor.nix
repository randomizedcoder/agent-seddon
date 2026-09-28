# nix/checks/rest-descriptor.nix
#
# Exercises the Envoy transcoder descriptor build (nix/rest-descriptor.nix) on the
# gate: the `buf --as-file-descriptor-set` invocation must succeed, and the derived
# `services.txt` must be a non-trivial, all-`agent.v1.*` list. The rendered transcoder
# listener itself is validated end-to-end by the `portal-envoy` check (`envoy --mode
# validate` over the real spec, which now carries the transcoder), and round-tripped by
# the opt-in `nix run .#rest-integration` (increment 06). docs/design/rest-openapi/.
{
  pkgs,
  versions,
}:
let
  restDescriptor = import ../rest-descriptor.nix { inherit pkgs versions; };
in
pkgs.runCommand "rest-descriptor-check" { } ''
  if [ ! -s ${restDescriptor}/agent_descriptor.pb ]; then
    echo "rest-descriptor: agent_descriptor.pb is missing or empty" >&2
    exit 1
  fi

  n="$(wc -l < ${restDescriptor}/services.txt)"
  # The surface is ~40 agent.v1 services; a build that collapsed the extraction would
  # dip well below this floor. A generous floor (not an exact count) so adding a
  # service never fails the gate — the point is "the list is populated", not its size.
  if [ "$n" -lt 30 ]; then
    echo "rest-descriptor: expected >=30 transcoded services, got $n" >&2
    cat ${restDescriptor}/services.txt >&2
    exit 1
  fi

  # Every entry must be a fully-qualified agent.v1 service — a non-agent name leaking
  # in (e.g. a google.api service, or an unversioned package) is a build-shape bug.
  if grep -vqE '^agent\.v1\.[A-Za-z][A-Za-z0-9]*$' ${restDescriptor}/services.txt; then
    echo "rest-descriptor: non-agent.v1 service leaked into the transcoder list:" >&2
    grep -vnE '^agent\.v1\.[A-Za-z][A-Za-z0-9]*$' ${restDescriptor}/services.txt >&2
    exit 1
  fi

  touch $out
''
