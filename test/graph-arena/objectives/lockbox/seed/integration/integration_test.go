// Package integration is the SHIPPED test suite for the lockbox objective —
// the agent must make `go test ./...` pass WITHOUT modifying this file.
// It builds the module's root binary and drives it as separate processes, so
// persistence is exercised for real and the agent keeps full freedom over the
// program's internal structure.
package integration

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

// build compiles the root package once per test run.
func build(t *testing.T) string {
	t.Helper()
	bin := filepath.Join(t.TempDir(), "lockbox")
	cmd := exec.Command("go", "build", "-o", bin, "..")
	cmd.Env = os.Environ()
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("go build failed: %v\n%s", err, out)
	}
	return bin
}

type result struct {
	stdout string
	stderr string
	code   int
}

func run(t *testing.T, bin string, args ...string) result {
	t.Helper()
	cmd := exec.Command(bin, args...)
	var so, se strings.Builder
	cmd.Stdout = &so
	cmd.Stderr = &se
	err := cmd.Run()
	code := 0
	if ee, ok := err.(*exec.ExitError); ok {
		code = ee.ExitCode()
	} else if err != nil {
		t.Fatalf("running %v: %v", args, err)
	}
	return result{stdout: so.String(), stderr: se.String(), code: code}
}

func TestSetGetPersistsAcrossInvocations(t *testing.T) {
	bin := build(t)
	db := filepath.Join(t.TempDir(), "t.db")
	if r := run(t, bin, "--db", db, "set", "alpha", "one"); r.code != 0 {
		t.Fatalf("set: exit %d, stderr %q", r.code, r.stderr)
	}
	r := run(t, bin, "--db", db, "get", "alpha")
	if r.code != 0 || r.stdout != "one\n" {
		t.Fatalf("get: exit %d, stdout %q", r.code, r.stdout)
	}
}

func TestMissingKeyExitsTwoWithNotFound(t *testing.T) {
	bin := build(t)
	db := filepath.Join(t.TempDir(), "t.db")
	for _, sub := range []string{"get", "delete"} {
		r := run(t, bin, "--db", db, sub, "nope")
		if r.code != 2 {
			t.Fatalf("%s missing: exit %d, want 2", sub, r.code)
		}
		if strings.TrimRight(r.stderr, "\n") != "not found" {
			t.Fatalf("%s missing: stderr %q, want `not found`", sub, r.stderr)
		}
	}
}

func TestInvalidKeyExitsOne(t *testing.T) {
	bin := build(t)
	db := filepath.Join(t.TempDir(), "t.db")
	r := run(t, bin, "--db", db, "set", "bad/key", "v")
	if r.code != 1 {
		t.Fatalf("invalid key: exit %d, want 1", r.code)
	}
	if !strings.Contains(r.stderr, "invalid key") {
		t.Fatalf("invalid key: stderr %q must mention `invalid key`", r.stderr)
	}
}

func TestListIsSorted(t *testing.T) {
	bin := build(t)
	db := filepath.Join(t.TempDir(), "t.db")
	for _, kv := range [][2]string{{"bravo", "2"}, {"alpha", "1"}, {"charlie", "3"}} {
		if r := run(t, bin, "--db", db, "set", kv[0], kv[1]); r.code != 0 {
			t.Fatalf("set %s: exit %d", kv[0], r.code)
		}
	}
	r := run(t, bin, "--db", db, "list")
	if r.code != 0 || r.stdout != "alpha\nbravo\ncharlie\n" {
		t.Fatalf("list: exit %d, stdout %q", r.code, r.stdout)
	}
}

func TestDeleteRemoves(t *testing.T) {
	bin := build(t)
	db := filepath.Join(t.TempDir(), "t.db")
	run(t, bin, "--db", db, "set", "alpha", "one")
	if r := run(t, bin, "--db", db, "delete", "alpha"); r.code != 0 {
		t.Fatalf("delete: exit %d", r.code)
	}
	if r := run(t, bin, "--db", db, "get", "alpha"); r.code != 2 {
		t.Fatalf("get after delete: exit %d, want 2", r.code)
	}
}
