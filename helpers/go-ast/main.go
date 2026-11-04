// Command agent-go-ast is the stdlib-only Go call-graph extractor for the
// agent-seddon code-review flow (component 06). It walks a repo, parses every
// .go file with go/parser (no type-checking, no external deps — so it works on
// any tree, even one that doesn't compile or has no modules fetched), and emits
// a compact JSON call graph on stdout:
//
//	{ "nodes": [...], "edges": [...], "packages": [...], "truncated": bool }
//
// A node is a top-level function/method; an edge is a *syntactic*, name-resolved
// caller→callee link between two in-repo functions (calls to the stdlib / other
// modules have no node, so they produce no edge). Names only: a call `x.Close()`
// links to every in-repo function or method named `Close`. The Rust side marks
// which nodes the diff changed and renders the blast radius — this program is
// deterministic and side-effect-free.
//
// Usage: agent-go-ast --root <dir>
//
// Focus: when AGENT_GO_AST_FOCUS holds a newline-separated list of repo-relative
// paths (the change's .go files), the whole repo is still parsed, but output is
// emitted in priority order until the caps: the focus files' functions, their
// direct callers and callees, the callers of those callers, then the rest. So on a
// repo larger than the caps the changed functions keep their edges. The paths are
// only compared with walked paths, never opened.
//
// Each node also says whether its file is generated (a `// Code generated … DO NOT
// EDIT.` line before the package clause, the Go convention), whether it is in a
// `_test.go` file, and its last line.
//
// The output is consumed by an untrusting parser, so it is bounded: caps on files,
// nodes, and edges (past a cap, `truncated` is set and the rest is dropped).
package main

import (
	"encoding/json"
	"flag"
	"go/ast"
	"go/parser"
	"go/token"
	"io/fs"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
)

const (
	maxFiles = 5000
	// Caps on what is emitted.
	maxNodes = 10000
	maxEdges = 50000
	// Cap on functions held while parsing (the emitted set is chosen from these).
	maxScanNodes = 200000
	// Caps on the focus list read from the environment.
	maxFocusPaths = 4000
	maxFocusBytes = 256 * 1024
	focusEnv      = "AGENT_GO_AST_FOCUS"
)

// generatedRe is the Go convention for generated files (`go help generate`).
var generatedRe = regexp.MustCompile(`^// Code generated .* DO NOT EDIT\.$`)

type node struct {
	ID        int    `json:"id"`
	Package   string `json:"package"`
	Name      string `json:"name"`
	Exported  bool   `json:"exported"`
	File      string `json:"file"`
	Line      int    `json:"line"`
	EndLine   int    `json:"end_line"`
	Test      bool   `json:"test,omitempty"`
	Generated bool   `json:"generated,omitempty"`
}

type edge struct {
	CallerID int `json:"caller_id"`
	CalleeID int `json:"callee_id"`
}

type pkgShape struct {
	Package     string `json:"package"`
	Files       uint32 `json:"files"`
	ExportedFns uint32 `json:"exported_fns"`
	Types       uint32 `json:"types"`
}

type output struct {
	Nodes     []node     `json:"nodes"`
	Edges     []edge     `json:"edges"`
	Packages  []pkgShape `json:"packages"`
	Truncated bool       `json:"truncated"`
}

func main() {
	root := flag.String("root", ".", "repository root to scan")
	flag.Parse()

	out := analyze(*root, parseFocus(os.Getenv(focusEnv)), maxNodes, maxEdges)
	// Emit compact JSON. A marshal error is unrecoverable; exit non-zero so the
	// caller records a failed run rather than parsing partial output.
	enc := json.NewEncoder(os.Stdout)
	if err := enc.Encode(out); err != nil {
		os.Exit(1)
	}
}

// parseFocus reads the focus list: bounded, blank lines dropped, each entry
// cleaned to a slash path. An absolute path or one that climbs out (`..`) can
// never equal a walked repo-relative path, so it is dropped rather than kept.
func parseFocus(s string) map[string]bool {
	if len(s) > maxFocusBytes {
		s = s[:maxFocusBytes]
	}
	focus := map[string]bool{}
	for _, line := range strings.Split(s, "\n") {
		if len(focus) >= maxFocusPaths {
			break
		}
		p := strings.TrimSpace(line)
		if p == "" || strings.HasPrefix(p, "/") {
			continue
		}
		p = filepath.ToSlash(filepath.Clean(p))
		if p == ".." || strings.HasPrefix(p, "../") {
			continue
		}
		focus[p] = true
	}
	return focus
}

// isGenerated reports whether a comment before the package clause marks the file
// generated.
func isGenerated(f *ast.File) bool {
	for _, cg := range f.Comments {
		if cg.Pos() >= f.Package {
			break
		}
		for _, c := range cg.List {
			if generatedRe.MatchString(c.Text) {
				return true
			}
		}
	}
	return false
}

// analyze parses the tree and emits at most nodeCap nodes and edgeCap edges
// (parameters so the tests can exercise the caps on a tiny tree).
func analyze(root string, focus map[string]bool, nodeCap, edgeCap int) output {
	fset := token.NewFileSet()
	var nodes []node
	var calls [][]string // calls[i] = callee names referenced by nodes[i]
	pkgAgg := map[string]*pkgShape{}
	var truncated bool
	files := 0

	_ = filepath.WalkDir(root, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return nil // unreadable entry — skip, never abort the walk
		}
		if d.IsDir() {
			// Skip VCS, vendored, and testdata trees — noise / not the repo's code.
			name := d.Name()
			if path != root && (name == ".git" || name == "vendor" || name == "testdata" || name == "node_modules") {
				return filepath.SkipDir
			}
			return nil
		}
		if truncated || files >= maxFiles {
			truncated = true
			return nil
		}
		if filepath.Ext(path) != ".go" {
			return nil
		}
		files++

		f, perr := parser.ParseFile(fset, path, nil, parser.SkipObjectResolution|parser.ParseComments)
		if perr != nil {
			return nil // a file that doesn't parse contributes nothing — fail-soft
		}
		rel, rerr := filepath.Rel(root, path)
		if rerr != nil {
			rel = path
		}
		rel = filepath.ToSlash(rel)
		pkgKey := filepath.ToSlash(filepath.Dir(rel))
		if pkgKey == "." {
			pkgKey = ""
		}
		generated := isGenerated(f)
		test := strings.HasSuffix(rel, "_test.go")

		shape := pkgAgg[pkgKey]
		if shape == nil {
			shape = &pkgShape{Package: pkgKey}
			pkgAgg[pkgKey] = shape
		}
		shape.Files++

		for _, decl := range f.Decls {
			switch dd := decl.(type) {
			case *ast.FuncDecl:
				if len(nodes) >= maxScanNodes {
					truncated = true
					continue
				}
				name := dd.Name.Name
				exported := ast.IsExported(name)
				if exported {
					shape.ExportedFns++
				}
				nodes = append(nodes, node{
					Package:   pkgKey,
					Name:      name,
					Exported:  exported,
					File:      rel,
					Line:      fset.Position(dd.Pos()).Line,
					EndLine:   fset.Position(dd.End()).Line,
					Test:      test,
					Generated: generated,
				})
				calls = append(calls, calleeNames(dd))
			case *ast.GenDecl:
				if dd.Tok == token.TYPE {
					for _, spec := range dd.Specs {
						if _, ok := spec.(*ast.TypeSpec); ok {
							shape.Types++
						}
					}
				}
			}
		}
		return nil
	})

	order, rank := priority(nodes, calls, focus)
	if len(order) > nodeCap {
		order = order[:nodeCap]
		truncated = true
	}
	// Emitted ids are positions in the emitted order.
	newID := make(map[int]int, len(order))
	emitted := make([]node, 0, len(order))
	for i, old := range order {
		n := nodes[old]
		n.ID = i
		newID[old] = i
		emitted = append(emitted, n)
	}
	edges := resolveEdges(order, calls, nodes, newID, rank, edgeCap, &truncated)

	packages := make([]pkgShape, 0, len(pkgAgg))
	for _, s := range pkgAgg {
		packages = append(packages, *s)
	}
	sort.Slice(packages, func(i, j int) bool { return packages[i].Package < packages[j].Package })

	return output{Nodes: emitted, Edges: edges, Packages: packages, Truncated: truncated}
}

// Priority ranks: the focus files' functions, their direct callers and callees,
// the callers of those callers, then everything else.
const (
	rankFocus = iota
	rankHop1
	rankHop2
	rankRest
)

// priority orders node indices by rank (stable: walk order within a rank) and
// returns each node's rank. With no focus every node is rankRest, so the order is
// the walk order, as before.
func priority(nodes []node, calls [][]string, focus map[string]bool) ([]int, []int) {
	rank := make([]int, len(nodes))
	for i := range rank {
		rank[i] = rankRest
	}
	byName := map[string][]int{}
	for i, n := range nodes {
		byName[n.Name] = append(byName[n.Name], i)
	}
	var level []int
	for i, n := range nodes {
		if focus[n.File] {
			rank[i] = rankFocus
			level = append(level, i)
		}
	}
	// Callers of a set: the nodes whose calls name any of its functions.
	callersOf := func(set []int) []int {
		names := map[string]bool{}
		for _, i := range set {
			names[nodes[i].Name] = true
		}
		var out []int
		for u, cs := range calls {
			for _, c := range cs {
				if names[c] {
					out = append(out, u)
					break
				}
			}
		}
		return out
	}
	mark := func(set []int, r int) []int {
		var fresh []int
		for _, i := range set {
			if rank[i] > r {
				rank[i] = r
				fresh = append(fresh, i)
			}
		}
		return fresh
	}
	if len(level) > 0 {
		// Each node joins hop1 at most once: callee names are deduped first, so a
		// name called from many sites (or shared by many fns) costs its fns once,
		// and the work is bounded by edges + nodes, not their product.
		var hop1 []int
		seen := map[int]bool{}
		add := func(i int) {
			if !seen[i] {
				seen[i] = true
				hop1 = append(hop1, i)
			}
		}
		callees := map[string]bool{}
		for _, f := range level {
			for _, c := range calls[f] {
				callees[c] = true
			}
		}
		for c := range callees {
			for _, i := range byName[c] {
				add(i)
			}
		}
		for _, i := range callersOf(level) {
			add(i)
		}
		hop1 = mark(hop1, rankHop1)
		mark(callersOf(hop1), rankHop2)
	}
	order := make([]int, len(nodes))
	for i := range order {
		order[i] = i
	}
	sort.SliceStable(order, func(a, b int) bool { return rank[order[a]] < rank[order[b]] })
	return order, rank
}

// calleeNames returns the distinct callee identifiers referenced in a function
// body: a bare `Foo()` (Ident) or the selector of `x.Foo()` (SelectorExpr).
func calleeNames(fn *ast.FuncDecl) []string {
	if fn.Body == nil {
		return nil
	}
	seen := map[string]struct{}{}
	var names []string
	ast.Inspect(fn.Body, func(n ast.Node) bool {
		call, ok := n.(*ast.CallExpr)
		if !ok {
			return true
		}
		var name string
		switch fun := call.Fun.(type) {
		case *ast.Ident:
			name = fun.Name
		case *ast.SelectorExpr:
			name = fun.Sel.Name
		}
		if name != "" {
			if _, dup := seen[name]; !dup {
				seen[name] = struct{}{}
				names = append(names, name)
			}
		}
		return true
	})
	return names
}

// resolveEdges turns the emitted nodes' callee names into caller→callee edges
// between emitted nodes, de-duplicated, self-loops dropped. An edge's priority is
// the better rank of its two ends. At most edgeCap are held: past it, a better
// edge evicts one from the worst non-empty bucket, so the focus functions' edges
// survive the cap and memory stays bounded. Emitted best priority first.
func resolveEdges(order []int, calls [][]string, nodes []node, newID map[int]int, rank []int, edgeCap int, truncated *bool) []edge {
	byName := map[string][]int{}
	for _, old := range order {
		byName[nodes[old].Name] = append(byName[nodes[old].Name], old)
	}
	buckets := make([][]edge, rankRest+1)
	total := 0
	dedup := map[[2]int]struct{}{}
	for _, caller := range order {
		for _, name := range calls[caller] {
			for _, callee := range byName[name] {
				if callee == caller {
					continue // ignore self-recursion — no blast-radius signal
				}
				key := [2]int{newID[caller], newID[callee]}
				if _, dup := dedup[key]; dup {
					continue
				}
				r := rank[caller]
				if rank[callee] < r {
					r = rank[callee]
				}
				if total >= edgeCap {
					*truncated = true
					if !evictWorse(buckets, r, dedup) {
						continue
					}
					total--
				}
				dedup[key] = struct{}{}
				buckets[r] = append(buckets[r], edge{CallerID: key[0], CalleeID: key[1]})
				total++
			}
		}
	}
	edges := make([]edge, 0, total)
	for _, b := range buckets {
		edges = append(edges, b...)
	}
	return edges
}

// evictWorse drops the last edge of the worst bucket ranked below r, if any, and
// reports whether it did.
func evictWorse(buckets [][]edge, r int, dedup map[[2]int]struct{}) bool {
	for w := len(buckets) - 1; w > r; w-- {
		if n := len(buckets[w]); n > 0 {
			e := buckets[w][n-1]
			delete(dedup, [2]int{e.CallerID, e.CalleeID})
			buckets[w] = buckets[w][:n-1]
			return true
		}
	}
	return false
}
