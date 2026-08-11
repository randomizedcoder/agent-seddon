// FAIL fixture (fail-tests-pass): malformed report wording drifts from the contract
// Reference solution for the logtriage objective — the PASS fixture (R13).
package main

import (
	"bufio"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

type stats struct {
	levels    map[string]int
	buckets   map[string]int
	malformed int
}

func newStats() *stats {
	return &stats{levels: map[string]int{}, buckets: map[string]int{}}
}

func (s *stats) scan(path string) error {
	f, err := os.Open(path)
	if err != nil {
		return err
	}
	defer f.Close()
	sc := bufio.NewScanner(f)
	sc.Buffer(make([]byte, 0, 64*1024), 1024*1024)
	for sc.Scan() {
		parts := strings.SplitN(sc.Text(), " ", 3)
		if len(parts) < 3 {
			s.malformed++
			continue
		}
		ts, err := time.Parse(time.RFC3339, parts[0])
		if err != nil || parts[1] != strings.ToUpper(parts[1]) || parts[1] == "" {
			s.malformed++
			continue
		}
		s.levels[parts[1]]++
		s.buckets[ts.UTC().Format("2006-01-02T15")]++
	}
	return sc.Err()
}

func fail(code int, msg string) {
	fmt.Fprintln(os.Stderr, msg)
	os.Exit(code)
}

func emit(m map[string]int, format string) {
	if format == "json" {
		b, _ := json.Marshal(m)
		fmt.Println(string(b))
		return
	}
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	for _, k := range keys {
		fmt.Printf("%s %d\n", k, m[k])
	}
}

func main() {
	input := flag.String("input", "", "log file ('-' = stdin unsupported here)")
	config := flag.String("config", "", "config file with include= lines")
	format := flag.String("format", "text", "text | json")
	flag.Parse()
	if flag.NArg() != 1 {
		fail(1, "usage: logtriage [--input FILE | --config CFG] [--format json] levels|buckets")
	}
	sub := flag.Arg(0)
	if sub != "levels" && sub != "buckets" {
		fail(1, "unknown subcommand: "+sub)
	}
	st := newStats()
	var inputs []string
	if *config != "" {
		cfgDir := filepath.Dir(*config)
		f, err := os.Open(*config)
		if err != nil {
			fail(1, err.Error())
		}
		sc := bufio.NewScanner(f)
		for sc.Scan() {
			line := strings.TrimSpace(sc.Text())
			if p, ok := strings.CutPrefix(line, "include="); ok {
				// Include safety: never step outside the config's directory.
				if filepath.IsAbs(p) || strings.Contains(p, "..") {
					fail(1, "unsafe include")
				}
				inputs = append(inputs, filepath.Join(cfgDir, p))
			}
		}
		f.Close()
	}
	if *input != "" {
		inputs = append(inputs, *input)
	}
	if len(inputs) == 0 {
		fail(1, "no input: pass --input or --config")
	}
	for _, p := range inputs {
		if err := st.scan(p); err != nil {
			fail(1, err.Error())
		}
	}
	if st.malformed > 0 {
		fmt.Fprintf(os.Stderr, "ignored %d lines\n", st.malformed)
	}
	if sub == "levels" {
		emit(st.levels, *format)
	} else {
		emit(st.buckets, *format)
	}
}
