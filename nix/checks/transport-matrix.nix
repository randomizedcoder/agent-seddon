# nix/checks/transport-matrix.nix
#
# Executes the Matrix `MessageTransport` impl's tests + the matrix arm of the
# transport-kind builder (`agent_slack::kind`), which sit behind the non-default
# `transport-matrix` cargo feature (config design C37, increment D2b,
# docs/design/config/05-message-transport.md). The main `test` check runs *default*
# features (Slack only), so this dedicated, feature-scoped check EXECUTES the matrix
# room-id path-encoding + response-classify + no-token matrix matrix, plus the
# card->transport build through the ONE factory — the D2b twin of forge-gitea.
#
# Matrix is opt-in (a second transport host: PUT-with-transaction-id client-server
# API, room id in the URL path, single access token, `errcode` errors); enabling the
# feature keeps Slack on too, so `known_kinds()` asserts both. Hermetic: the tests are
# pure URL/JSON mapping + the no-token early-error path (no live homeserver).
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-slack --features transport-matrix";
  }
)
