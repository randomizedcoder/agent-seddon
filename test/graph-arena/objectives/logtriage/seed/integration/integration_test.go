// Package integration is the SHIPPED suite for the logtriage objective — the
// agent must make `go test ./...` pass WITHOUT modifying this file. It builds
// the module root and drives the binary as processes (structure stays free).
package integration

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func build(t *testing.T) string {
	t.Helper()
	bin := filepath.Join(t.TempDir(), "logtriage")
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

// The seed corpus lives next to the module root.
func sample(t *testing.T, name string) string {
	t.Helper()
	p, err := filepath.Abs(filepath.Join("..", "testdata", name))
	if err != nil {
		t.Fatal(err)
	}
	return p
}

func TestLevelsSortedCounts(t *testing.T) {
	bin := build(t)
	r := run(t, bin, "--input", sample(t, "sample.log"), "levels")
	if r.code != 0 {
		t.Fatalf("levels: exit %d, stderr %q", r.code, r.stderr)
	}
	if r.stdout != "ERROR 3\nINFO 3\nWARN 1\n" {
		t.Fatalf("levels: stdout %q", r.stdout)
	}
}

func TestBucketsHourly(t *testing.T) {
	bin := build(t)
	r := run(t, bin, "--input", sample(t, "sample.log"), "buckets")
	if r.code != 0 || r.stdout != "2026-08-11T10 3\n2026-08-11T11 3\n2026-08-11T12 1\n" {
		t.Fatalf("buckets: exit %d, stdout %q", r.code, r.stdout)
	}
}

func TestMalformedSkippedReportedExitZero(t *testing.T) {
	bin := build(t)
	r := run(t, bin, "--input", sample(t, "sample.log"), "levels")
	if r.code != 0 || !strings.Contains(r.stderr, "skipped 1 malformed") {
		t.Fatalf("malformed: exit %d, stderr %q", r.code, r.stderr)
	}
}

func TestIncludeMergesAndTraversalRejected(t *testing.T) {
	bin := build(t)
	r := run(t, bin, "--config", sample(t, "merge.cfg"), "levels")
	if r.code != 0 || r.stdout != "ERROR 3\nINFO 4\nWARN 2\n" {
		t.Fatalf("merge: exit %d, stdout %q", r.code, r.stdout)
	}
	r = run(t, bin, "--config", sample(t, "evil.cfg"), "levels")
	if r.code != 1 || !strings.Contains(r.stderr, "unsafe include") {
		t.Fatalf("evil include: exit %d, stderr %q", r.code, r.stderr)
	}
}

func TestJSONOutput(t *testing.T) {
	bin := build(t)
	r := run(t, bin, "--input", sample(t, "sample.log"), "--format", "json", "levels")
	if r.code != 0 {
		t.Fatalf("json: exit %d", r.code)
	}
	for _, want := range []string{`"ERROR":3`, `"INFO":3`, `"WARN":1`} {
		if !strings.Contains(strings.ReplaceAll(r.stdout, " ", ""), want) {
			t.Fatalf("json levels: stdout %q missing %s", r.stdout, want)
		}
	}
}

// TestPerf: 1M lines under 5s (the perf requirement runs exactly this test).
func TestPerf(t *testing.T) {
	bin := build(t)
	big := filepath.Join(t.TempDir(), "big.log")
	f, err := os.Create(big)
	if err != nil {
		t.Fatal(err)
	}
	levels := []string{"INFO", "WARN", "ERROR"}
	for i := range 1_000_000 {
		fmt.Fprintf(f, "2026-08-11T%02d:00:00Z %s line %d\n", 10+(i%12), levels[i%3], i)
	}
	f.Close()
	start := time.Now()
	r := run(t, bin, "--input", big, "levels")
	elapsed := time.Since(start)
	if r.code != 0 {
		t.Fatalf("perf run: exit %d", r.code)
	}
	if elapsed > 5*time.Second {
		t.Fatalf("1M lines took %v (budget 5s)", elapsed)
	}
}
