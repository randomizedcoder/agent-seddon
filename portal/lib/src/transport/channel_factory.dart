// The transport abstraction — the one place native and web differ.
//
// Browsers cannot speak raw gRPC (HTTP/2 trailers), so the web build dials a
// grpc-web proxy while native dials the gateway directly. Conditional imports pick
// the right `createGatewayChannel` at compile time; every service client is built
// from the single channel it returns, so all UI + client code is shared.
export 'channel_stub.dart'
    if (dart.library.io) 'channel_io.dart'
    if (dart.library.html) 'channel_web.dart';
