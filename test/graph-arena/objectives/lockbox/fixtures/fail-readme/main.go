// FAIL fixture (fail-readme): correct program, README.md missing
// Reference solution for the lockbox objective — the PASS fixture proving
// every requirement's check can succeed (check-the-checks, R13).
package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"regexp"
	"sort"
	"strings"
)

var keyRe = regexp.MustCompile(`^[A-Za-z0-9_.-]+$`)

func load(path string) (map[string]string, error) {
	m := map[string]string{}
	f, err := os.Open(path)
	if os.IsNotExist(err) {
		return m, nil
	}
	if err != nil {
		return nil, err
	}
	defer f.Close()
	sc := bufio.NewScanner(f)
	for sc.Scan() {
		if k, v, ok := strings.Cut(sc.Text(), "="); ok {
			m[k] = v
		}
	}
	return m, sc.Err()
}

func save(path string, m map[string]string) error {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	var b strings.Builder
	for _, k := range keys {
		fmt.Fprintf(&b, "%s=%s\n", k, m[k])
	}
	return os.WriteFile(path, []byte(b.String()), 0o644)
}

func fail(code int, msg string) {
	fmt.Fprintln(os.Stderr, msg)
	os.Exit(code)
}

func main() {
	db := flag.String("db", "lockbox.db", "database file")
	flag.Parse()
	args := flag.Args()
	if len(args) == 0 {
		fail(1, "usage: lockbox --db FILE set|get|delete|list [KEY] [VALUE]")
	}
	store, err := load(*db)
	if err != nil {
		fail(1, err.Error())
	}
	checkKey := func(k string) {
		if !keyRe.MatchString(k) {
			fail(1, "invalid key")
		}
	}
	switch args[0] {
	case "set":
		if len(args) != 3 {
			fail(1, "usage: set KEY VALUE")
		}
		checkKey(args[1])
		store[args[1]] = args[2]
		if err := save(*db, store); err != nil {
			fail(1, err.Error())
		}
	case "get":
		if len(args) != 2 {
			fail(1, "usage: get KEY")
		}
		checkKey(args[1])
		v, ok := store[args[1]]
		if !ok {
			fail(2, "not found")
		}
		fmt.Println(v)
	case "delete":
		if len(args) != 2 {
			fail(1, "usage: delete KEY")
		}
		checkKey(args[1])
		if _, ok := store[args[1]]; !ok {
			fail(2, "not found")
		}
		delete(store, args[1])
		if err := save(*db, store); err != nil {
			fail(1, err.Error())
		}
	case "list":
		keys := make([]string, 0, len(store))
		for k := range store {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		for _, k := range keys {
			fmt.Println(k)
		}
	default:
		fail(1, "unknown subcommand: "+args[0])
	}
}
