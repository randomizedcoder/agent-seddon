package integration

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// waitFileContains polls path until every want substring is present (accepted
// publishes are flushed per line, but the write races the PUB reply).
func waitFileContains(t *testing.T, path string, want ...string) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for {
		data, err := os.ReadFile(path)
		if err == nil {
			all := true
			for _, w := range want {
				if !strings.Contains(string(data), w) {
					all = false
				}
			}
			if all {
				return
			}
		}
		if time.Now().After(deadline) {
			data, _ := os.ReadFile(path)
			t.Fatalf("journal %s never contained %q; content:\n%s", path, want, data)
		}
		time.Sleep(20 * time.Millisecond)
	}
}

func TestJournal(t *testing.T) {
	bin := build(t)
	journal := filepath.Join(t.TempDir(), "journal.log")
	s := startServer(t, bin, "--journal", journal)

	c := authed(t, s.Addr)
	c.send(t, "PUB logs alpha line")
	c.expect(t, "OK")
	c.send(t, "PUB logs beta line")
	c.expect(t, "OK")

	waitFileContains(t, journal, "logs alpha line", "logs beta line")
}

func TestReplay(t *testing.T) {
	bin := build(t)
	journal := filepath.Join(t.TempDir(), "journal.log")
	s := startServer(t, bin, "--journal", journal)

	pub := authed(t, s.Addr)
	for _, m := range []string{"m one", "m two", "m three"} {
		pub.send(t, "PUB t "+m)
		pub.expect(t, "OK")
	}
	waitFileContains(t, journal, "m three")

	// Last 2, oldest first, then OK.
	c := authed(t, s.Addr)
	c.send(t, "REPLAY t 2")
	c.expect(t, "MSG t m two")
	c.expect(t, "MSG t m three")
	c.expect(t, "OK")

	// Replay filters by topic: an unknown topic replays nothing but still OKs.
	c.send(t, "REPLAY empty-topic 5")
	c.expect(t, "OK")
}

func TestReplayRestart(t *testing.T) {
	bin := build(t)
	journal := filepath.Join(t.TempDir(), "journal.log")
	s := startServer(t, bin, "--journal", journal)

	pub := authed(t, s.Addr)
	pub.send(t, "PUB t survives one")
	pub.expect(t, "OK")
	pub.send(t, "PUB t survives two")
	pub.expect(t, "OK")
	waitFileContains(t, journal, "survives two")
	s.Stop()

	// A fresh process on the same journal must serve pre-restart messages.
	s2 := startServer(t, bin, "--journal", journal)
	c := authed(t, s2.Addr)
	c.send(t, "REPLAY t 5")
	c.expect(t, "MSG t survives one")
	c.expect(t, "MSG t survives two")
	c.expect(t, "OK")
}
