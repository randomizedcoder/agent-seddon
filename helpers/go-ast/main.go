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
// modules have no node, so they produce no edge). The Rust side marks which nodes
// the diff changed and renders the blast radius — this program is deterministic
// and side-effect-free.
//
// Usage: agent-go-ast --root <dir>
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
	"sort"
)

const (
	maxFiles = 5000
	maxNodes = 10000
	maxEdges = 50000
)

type node struct {
	ID       int    `json:"id"`
	Package  string `json:"package"`
	Name     string `json:"name"`
	Exported bool   `json:"exported"`
	File     string `json:"file"`
	Line     int    `json:"line"`
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

	out := analyze(*root)
	// Emit compact JSON. A marshal error is unrecoverable; exit non-zero so the
	// caller records a failed run rather than parsing partial output.
	enc := json.NewEncoder(os.Stdout)
	if err := enc.Encode(out); err != nil {
		os.Exit(1)
	}
}

func analyze(root string) output {
	fset := token.NewFileSet()
	var nodes []node
	var calls [][]string // calls[i] = callee names referenced by nodes[i]
	nameToIDs := map[string][]int{}
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

		f, perr := parser.ParseFile(fset, path, nil, parser.SkipObjectResolution)
		if perr != nil {
			return nil // a file that doesn't parse contributes nothing — fail-soft
		}
		rel, rerr := filepath.Rel(root, path)
		if rerr != nil {
			rel = path
		}
		pkgKey := filepath.ToSlash(filepath.Dir(rel))
		if pkgKey == "." {
			pkgKey = ""
		}

		shape := pkgAgg[pkgKey]
		if shape == nil {
			shape = &pkgShape{Package: pkgKey}
			pkgAgg[pkgKey] = shape
		}
		shape.Files++

		for _, decl := range f.Decls {
			switch dd := decl.(type) {
			case *ast.FuncDecl:
				if len(nodes) >= maxNodes {
					truncated = true
					continue
				}
				id := len(nodes)
				name := dd.Name.Name
				exported := ast.IsExported(name)
				if exported {
					shape.ExportedFns++
				}
				nodes = append(nodes, node{
					ID:       id,
					Package:  pkgKey,
					Name:     name,
					Exported: exported,
					File:     filepath.ToSlash(rel),
					Line:     fset.Position(dd.Pos()).Line,
				})
				calls = append(calls, calleeNames(dd))
				nameToIDs[name] = append(nameToIDs[name], id)
			case *ast.GenDecl:
				if dd.Tok.String() == "type" {
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

	// Resolve name-based edges. A callee name that matches no in-repo node (stdlib
	// / external) is silently dropped — we only graph intra-repo calls.
	edges := resolveEdges(calls, nameToIDs, &truncated)

	packages := make([]pkgShape, 0, len(pkgAgg))
	for _, s := range pkgAgg {
		packages = append(packages, *s)
	}
	sort.Slice(packages, func(i, j int) bool { return packages[i].Package < packages[j].Package })

	return output{Nodes: nodes, Edges: edges, Packages: packages, Truncated: truncated}
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

// resolveEdges turns per-node callee names into caller→callee edges via the name
// index, de-duplicated, self-loops dropped, capped at maxEdges.
func resolveEdges(calls [][]string, nameToIDs map[string][]int, truncated *bool) []edge {
	var edges []edge
	dedup := map[[2]int]struct{}{}
	for caller, names := range calls {
		for _, name := range names {
			for _, callee := range nameToIDs[name] {
				if callee == caller {
					continue // ignore self-recursion — no blast-radius signal
				}
				key := [2]int{caller, callee}
				if _, dup := dedup[key]; dup {
					continue
				}
				if len(edges) >= maxEdges {
					*truncated = true
					return edges
				}
				dedup[key] = struct{}{}
				edges = append(edges, edge{CallerID: caller, CalleeID: callee})
			}
		}
	}
	return edges
}
