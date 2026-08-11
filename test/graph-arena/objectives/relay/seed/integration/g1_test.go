package integration

import "testing"

func TestAuth(t *testing.T) {
	bin := build(t)
	s := startServer(t, bin)

	// A valid token authenticates.
	good := dialRelay(t, s.Addr)
	good.send(t, "AUTH alpha-token")
	good.expect(t, "OK")

	// An invalid token gets the exact error and the connection is closed.
	bad := dialRelay(t, s.Addr)
	bad.send(t, "AUTH not-a-token")
	bad.expect(t, "ERR unauthorized")
	bad.expectClosed(t)

	// Any command before AUTH is unauthorized and the connection is closed.
	sneak := dialRelay(t, s.Addr)
	sneak.send(t, "PING")
	sneak.expect(t, "ERR unauthorized")
	sneak.expectClosed(t)

	// The server is still serving after rejected connections.
	again := dialRelay(t, s.Addr)
	again.send(t, "AUTH bravo-token")
	again.expect(t, "OK")
}

func TestPubSub(t *testing.T) {
	bin := build(t)
	s := startServer(t, bin)

	sub := authed(t, s.Addr)
	sub.send(t, "SUB news")
	sub.expect(t, "OK")

	pub := authed(t, s.Addr)
	pub.send(t, "PUB news hello world with spaces")
	pub.expect(t, "OK")

	// The subscriber receives exactly the MSG line, text preserved verbatim.
	sub.expect(t, "MSG news hello world with spaces")

	// A topic the subscriber did not subscribe to is not delivered; a
	// subsequent subscribed publish arrives next — proving nothing leaked
	// in between.
	pub.send(t, "PUB other not for sub")
	pub.expect(t, "OK")
	pub.send(t, "PUB news second")
	pub.expect(t, "OK")
	sub.expect(t, "MSG news second")
}

func TestPing(t *testing.T) {
	bin := build(t)
	s := startServer(t, bin)
	c := authed(t, s.Addr)
	c.send(t, "PING")
	c.expect(t, "PONG")
}
