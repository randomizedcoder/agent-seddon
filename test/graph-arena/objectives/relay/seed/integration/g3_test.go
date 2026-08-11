package integration

import (
	"fmt"
	"io"
	"net/http"
	"regexp"
	"testing"
	"time"
)

func TestMetrics(t *testing.T) {
	bin := build(t)
	s := startServer(t, bin, "--metrics", "127.0.0.1:0")

	c := authed(t, s.Addr)
	c.send(t, "PUB m one")
	c.expect(t, "OK")
	c.send(t, "PUB m two")
	c.expect(t, "OK")

	// The counters race the PUB replies; poll briefly for the settled values.
	published := regexp.MustCompile(`(?m)^relay_published_total 2$`)
	connections := regexp.MustCompile(`(?m)^relay_connections_total [1-9]\d*$`)
	deadline := time.Now().Add(3 * time.Second)
	var body string
	for {
		resp, err := http.Get(fmt.Sprintf("http://%s/metrics", s.MetricsAddr))
		if err == nil {
			b, _ := io.ReadAll(resp.Body)
			_ = resp.Body.Close()
			body = string(b)
			if published.MatchString(body) && connections.MatchString(body) {
				return
			}
		}
		if time.Now().After(deadline) {
			t.Fatalf("metrics never settled; last body:\n%s", body)
		}
		time.Sleep(20 * time.Millisecond)
	}
}

func TestRateLimit(t *testing.T) {
	bin := build(t)
	s := startServer(t, bin, "--rate", "5")

	c := authed(t, s.Addr)
	const total = 20
	var ok, limited int
	for i := range total {
		c.send(t, fmt.Sprintf("PUB burst message %d", i))
		switch reply := c.recv(t); reply {
		case "OK":
			ok++
		case "ERR rate limited":
			limited++
		default:
			t.Fatalf("PUB reply = %q, want OK or ERR rate limited", reply)
		}
	}
	// Burst 5, refill 5/s, 20 back-to-back sends: the first 5 pass, and even
	// a generously slow loop cannot refill 15 tokens.
	if ok < 5 {
		t.Fatalf("only %d/%d publishes accepted — the burst budget must pass", ok, total)
	}
	if limited < 1 {
		t.Fatalf("no publish was rate limited across %d rapid sends", total)
	}
	// The connection survives being limited.
	c.send(t, "PING")
	c.expect(t, "PONG")
}

func TestMalformed(t *testing.T) {
	bin := build(t)
	s := startServer(t, bin)

	c := authed(t, s.Addr)
	c.send(t, "FROBNICATE the widget")
	c.expect(t, "ERR bad command")
	// Hostile bytes on the line: still the exact reply, connection still open.
	c.send(t, "\x00\x01garbage\x7f")
	c.expect(t, "ERR bad command")
	c.send(t, "PING")
	c.expect(t, "PONG")

	// And the PROCESS survives: a brand-new connection still authenticates.
	fresh := dialRelay(t, s.Addr)
	fresh.send(t, "AUTH alpha-token")
	fresh.expect(t, "OK")

	// A malformed PUB (missing text) is bad, not a crash.
	c.send(t, "PUB onlytopic")
	c.expect(t, "ERR bad command")
}
