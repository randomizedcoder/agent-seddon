// Package integration is the SHIPPED suite for the relay objective — the agent
// must make `go test ./...` pass WITHOUT modifying this package. It builds the
// module root and drives the binary as processes over real TCP connections
// (internal structure stays free).
package integration

import (
	"bufio"
	"fmt"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

const dialTimeout = 5 * time.Second

func build(t *testing.T) string {
	t.Helper()
	bin := filepath.Join(t.TempDir(), "relay")
	cmd := exec.Command("go", "build", "-o", bin, "..")
	cmd.Env = os.Environ()
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("go build failed: %v\n%s", err, out)
	}
	return bin
}

func tokensFile(t *testing.T) string {
	t.Helper()
	p, err := filepath.Abs(filepath.Join("..", "testdata", "tokens.txt"))
	if err != nil {
		t.Fatal(err)
	}
	return p
}

// server is one running relay process; Addr/MetricsAddr are parsed from the
// LISTENING/METRICS stdout lines.
type server struct {
	cmd         *exec.Cmd
	Addr        string
	MetricsAddr string

	mu   sync.Mutex
	done bool
}

// startServer launches the binary and waits for its address announcements.
// Extra args are appended after --listen/--tokens.
func startServer(t *testing.T, bin string, extra ...string) *server {
	t.Helper()
	args := append([]string{"--listen", "127.0.0.1:0", "--tokens", tokensFile(t)}, extra...)
	cmd := exec.Command(bin, args...)
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	cmd.Stderr = os.Stderr
	if err := cmd.Start(); err != nil {
		t.Fatalf("starting relay: %v", err)
	}
	s := &server{cmd: cmd}
	t.Cleanup(s.Stop)

	wantMetrics := false
	for _, a := range extra {
		if a == "--metrics" {
			wantMetrics = true
		}
	}
	got := make(chan error, 1)
	go func() {
		sc := bufio.NewScanner(stdout)
		announced := false
		// After announcing, keep draining so the child never blocks on a
		// full stdout pipe.
		for sc.Scan() {
			if announced {
				continue
			}
			line := sc.Text()
			if rest, ok := strings.CutPrefix(line, "LISTENING "); ok {
				s.Addr = strings.TrimSpace(rest)
			}
			if rest, ok := strings.CutPrefix(line, "METRICS "); ok {
				s.MetricsAddr = strings.TrimSpace(rest)
			}
			if s.Addr != "" && (!wantMetrics || s.MetricsAddr != "") {
				announced = true
				got <- nil
			}
		}
		if !announced {
			got <- fmt.Errorf(
				"relay exited before announcing LISTENING/METRICS (read error: %v)",
				sc.Err(),
			)
		}
	}()
	select {
	case err := <-got:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(10 * time.Second):
		t.Fatal("timeout waiting for LISTENING/METRICS announcement")
	}
	return s
}

// Stop kills the process (idempotent). Accepted publishes must already be
// durable (the journal is flushed per publish), so kill is fair.
func (s *server) Stop() {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.done {
		return
	}
	s.done = true
	_ = s.cmd.Process.Kill()
	_, _ = s.cmd.Process.Wait()
}

// client is one authenticated-or-not TCP connection speaking the line protocol.
type client struct {
	conn net.Conn
	r    *bufio.Reader
}

func dialRelay(t *testing.T, addr string) *client {
	t.Helper()
	conn, err := net.DialTimeout("tcp", addr, dialTimeout)
	if err != nil {
		t.Fatalf("dial %s: %v", addr, err)
	}
	t.Cleanup(func() { _ = conn.Close() })
	return &client{conn: conn, r: bufio.NewReader(conn)}
}

func (c *client) send(t *testing.T, line string) {
	t.Helper()
	_ = c.conn.SetWriteDeadline(time.Now().Add(dialTimeout))
	if _, err := c.conn.Write([]byte(line + "\n")); err != nil {
		t.Fatalf("send %q: %v", line, err)
	}
}

// recv reads one reply line (bounded wait).
func (c *client) recv(t *testing.T) string {
	t.Helper()
	_ = c.conn.SetReadDeadline(time.Now().Add(dialTimeout))
	line, err := c.r.ReadString('\n')
	if err != nil {
		t.Fatalf("recv: %v (partial %q)", err, line)
	}
	return strings.TrimRight(line, "\r\n")
}

// expect asserts the next reply line is exactly want.
func (c *client) expect(t *testing.T, want string) {
	t.Helper()
	if got := c.recv(t); got != want {
		t.Fatalf("reply = %q, want %q", got, want)
	}
}

// expectClosed asserts the server closed the connection (EOF on next read).
func (c *client) expectClosed(t *testing.T) {
	t.Helper()
	_ = c.conn.SetReadDeadline(time.Now().Add(dialTimeout))
	if line, err := c.r.ReadString('\n'); err == nil {
		t.Fatalf("connection still open, read %q", line)
	}
}

// authed dials and authenticates with a valid token.
func authed(t *testing.T, addr string) *client {
	t.Helper()
	c := dialRelay(t, addr)
	c.send(t, "AUTH alpha-token")
	c.expect(t, "OK")
	return c
}
