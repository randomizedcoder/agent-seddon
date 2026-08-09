// Command agent-go-graph is the type-aware Go code-graph extractor for the
// agent-seddon AstBackend seam (component: code-graph). Where the stdlib-only
// agent-go-ast is syntactic (name-resolved edges, runs on any tree even one that
// doesn't compile), this program loads full type information with
// golang.org/x/tools so it can resolve what syntax alone cannot:
//
//   - precise caller→callee call edges (CHA over the SSA form),
//   - the implicit interface-satisfaction relation (go/types.Implements) — the
//     Go-specific query nothing else answers cheaply,
//   - the package import graph (for dependency-path queries).
//
// It therefore needs the target to load / type-check. It is fail-soft: a package
// that doesn't type-check contributes whatever resolved plus a diagnostic, never an
// abort; SSA/callgraph construction is wrapped so a build failure degrades to
// "symbols + implements, no edges" rather than crashing.
//
// Usage: agent-go-graph --root <dir>
//
// Output is compact JSON on stdout, consumed by an untrusting Rust parser, so it is
// bounded (caps on every list → `truncated`):
//
//	{ "symbols": [...], "edges": [...], "implements": [...],
//	  "imports": [...], "packages": [...], "diagnostics": [...], "truncated": bool }
//
// A symbol is a top-level func/method/type/interface/struct with a stable id; an
// edge is a caller→callee link between two in-repo symbols; an `implements` entry
// links a concrete type to an interface it satisfies. This program is deterministic
// (symbols are assigned ids in sorted package order) and side-effect-free.
package main

import (
	"encoding/json"
	"flag"
	"go/ast"
	"go/types"
	"os"
	"path/filepath"
	"sort"

	"golang.org/x/tools/go/callgraph/cha"
	"golang.org/x/tools/go/packages"
	"golang.org/x/tools/go/ssa"
	"golang.org/x/tools/go/ssa/ssautil"
)

// Caps bound a hostile or generated repo. Past a cap, `truncated` is set and the
// rest is dropped — the Rust parser is untrusting but these keep the JSON itself
// bounded before it is even emitted.
const (
	maxSymbols    = 20000
	maxEdges      = 100000
	maxImplements = 20000
	maxImports    = 5000
	maxStr        = 256
	maxDiags      = 100
)

type symbol struct {
	ID       int    `json:"id"`
	Kind     string `json:"kind"` // func | method | interface | struct | type
	Name     string `json:"name"`
	Recv     string `json:"recv,omitempty"` // receiver type name, for methods
	Package  string `json:"package"`        // import path
	File     string `json:"file"`           // repo-relative, slash-separated
	Line     int    `json:"line"`
	Exported bool   `json:"exported"`
}

type edge struct {
	CallerID int `json:"caller_id"`
	CalleeID int `json:"callee_id"`
}

type implElem struct {
	TypeID      int `json:"type_id"`
	InterfaceID int `json:"interface_id"`
}

type importEdge struct {
	From string `json:"from"`
	To   string `json:"to"`
}

type pkgShape struct {
	Package     string `json:"package"` // import path
	Path        string `json:"path"`    // repo-relative dir
	Files       uint32 `json:"files"`
	ExportedFns uint32 `json:"exported_fns"`
	Types       uint32 `json:"types"`
}

type output struct {
	Symbols     []symbol     `json:"symbols"`
	Edges       []edge       `json:"edges"`
	Implements  []implElem   `json:"implements"`
	Imports     []importEdge `json:"imports"`
	Packages    []pkgShape   `json:"packages"`
	Diagnostics []string     `json:"diagnostics"`
	Truncated   bool         `json:"truncated"`
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

// interfaceRef pairs an interface symbol id with its underlying *types.Interface.
type interfaceRef struct {
	id    int
	iface *types.Interface
}

// namedRef pairs a concrete named-type symbol id with its *types.Named.
type namedRef struct {
	id    int
	named *types.Named
}

func analyze(root string) output {
	var out output

	absRoot, err := filepath.Abs(root)
	if err != nil {
		absRoot = root
	}

	cfg := &packages.Config{
		Mode: packages.NeedName | packages.NeedFiles | packages.NeedCompiledGoFiles |
			packages.NeedImports | packages.NeedDeps | packages.NeedTypes |
			packages.NeedSyntax | packages.NeedTypesInfo | packages.NeedModule,
		Dir:   absRoot,
		Tests: false,
	}
	pkgs, err := packages.Load(cfg, "./...")
	if err != nil {
		out.Diagnostics = append(out.Diagnostics, bound("load: "+err.Error()))
		return out
	}

	// Deterministic id assignment: sort roots by import path.
	sort.Slice(pkgs, func(i, j int) bool { return pkgs[i].PkgPath < pkgs[j].PkgPath })

	// Local (in-repo) package set — import edges are kept only between these, so a
	// dependency-path query stays inside the module.
	local := map[string]bool{}
	for _, p := range pkgs {
		if p.PkgPath != "" {
			local[p.PkgPath] = true
		}
	}

	objToID := map[types.Object]int{}
	var interfaces []interfaceRef
	var nameds []namedRef

	for _, p := range pkgs {
		for _, e := range p.Errors {
			if len(out.Diagnostics) < maxDiags {
				out.Diagnostics = append(out.Diagnostics, bound(p.PkgPath+": "+e.Msg))
			}
		}
		if p.TypesInfo == nil || p.Fset == nil {
			continue
		}

		shape := pkgShape{Package: p.PkgPath}
		fileSet := map[string]bool{}

		for _, file := range p.Syntax {
			for _, decl := range file.Decls {
				switch d := decl.(type) {
				case *ast.FuncDecl:
					if len(out.Symbols) >= maxSymbols {
						out.Truncated = true
						continue
					}
					obj := p.TypesInfo.Defs[d.Name]
					if obj == nil {
						continue
					}
					pos := p.Fset.Position(d.Pos())
					rel := relPath(absRoot, pos.Filename)
					fileSet[rel] = true
					kind := "func"
					recv := ""
					if d.Recv != nil && len(d.Recv.List) > 0 {
						kind = "method"
						recv = recvTypeName(d.Recv.List[0].Type)
					}
					exported := d.Name.IsExported()
					if exported && recv == "" {
						shape.ExportedFns++
					}
					id := len(out.Symbols)
					out.Symbols = append(out.Symbols, symbol{
						ID:       id,
						Kind:     kind,
						Name:     bound(d.Name.Name),
						Recv:     bound(recv),
						Package:  bound(p.PkgPath),
						File:     rel,
						Line:     pos.Line,
						Exported: exported,
					})
					objToID[obj] = id

				case *ast.GenDecl:
					if d.Tok.String() != "type" {
						continue
					}
					for _, spec := range d.Specs {
						ts, ok := spec.(*ast.TypeSpec)
						if !ok {
							continue
						}
						if len(out.Symbols) >= maxSymbols {
							out.Truncated = true
							continue
						}
						obj := p.TypesInfo.Defs[ts.Name]
						if obj == nil {
							continue
						}
						shape.Types++
						pos := p.Fset.Position(ts.Pos())
						rel := relPath(absRoot, pos.Filename)
						fileSet[rel] = true
						kind := "type"
						named, _ := obj.Type().(*types.Named)
						if named != nil {
							switch u := named.Underlying().(type) {
							case *types.Interface:
								kind = "interface"
								// Empty interfaces (interface{}) match everything —
								// skip them as implementation targets (still emitted
								// as symbols).
								if u.NumMethods() > 0 {
									interfaces = append(interfaces, interfaceRef{id: len(out.Symbols), iface: u})
								}
							case *types.Struct:
								kind = "struct"
								nameds = append(nameds, namedRef{id: len(out.Symbols), named: named})
							default:
								nameds = append(nameds, namedRef{id: len(out.Symbols), named: named})
							}
						}
						id := len(out.Symbols)
						out.Symbols = append(out.Symbols, symbol{
							ID:       id,
							Kind:     kind,
							Name:     bound(ts.Name.Name),
							Package:  bound(p.PkgPath),
							File:     rel,
							Line:     pos.Line,
							Exported: ts.Name.IsExported(),
						})
						objToID[obj] = id
					}
				}
			}
		}

		shape.Files = uint32(len(fileSet))
		shape.Path = pkgDir(absRoot, p)
		out.Packages = append(out.Packages, shape)

		// Import edges (intra-module only).
		if !out.Truncated && len(out.Imports) < maxImports {
			paths := make([]string, 0, len(p.Imports))
			for ip := range p.Imports {
				paths = append(paths, ip)
			}
			sort.Strings(paths)
			for _, ip := range paths {
				if !local[ip] {
					continue
				}
				if len(out.Imports) >= maxImports {
					out.Truncated = true
					break
				}
				out.Imports = append(out.Imports, importEdge{From: bound(p.PkgPath), To: bound(ip)})
			}
		}
	}

	out.Implements = resolveImplements(nameds, interfaces, &out.Truncated)
	buildEdges(pkgs, objToID, &out)

	return out
}

// resolveImplements records, for each concrete named type, every non-empty
// interface it satisfies (value or pointer receiver). Bounded by maxImplements.
func resolveImplements(nameds []namedRef, interfaces []interfaceRef, truncated *bool) []implElem {
	var impls []implElem
	for _, n := range nameds {
		if n.named == nil {
			continue
		}
		ptr := types.NewPointer(n.named)
		for _, i := range interfaces {
			if n.id == i.id || i.iface == nil {
				continue
			}
			if types.Implements(n.named, i.iface) || types.Implements(ptr, i.iface) {
				if len(impls) >= maxImplements {
					*truncated = true
					return impls
				}
				impls = append(impls, implElem{TypeID: n.id, InterfaceID: i.id})
			}
		}
	}
	return impls
}

// buildEdges resolves precise caller→callee edges via CHA over the SSA form. It is
// fail-soft: any panic during SSA build (an incomplete/non-compiling program)
// degrades to "no edges", never a crash.
func buildEdges(pkgs []*packages.Package, objToID map[types.Object]int, out *output) {
	defer func() { _ = recover() }()

	prog, _ := ssautil.AllPackages(pkgs, ssa.InstantiateGenerics)
	prog.Build()
	cg := cha.CallGraph(prog)

	seen := map[[2]int]bool{}
	// Deterministic order: collect edges then sort.
	for fn, node := range cg.Nodes {
		callerID, ok := ssaFnID(fn, objToID)
		if !ok {
			continue
		}
		for _, e := range node.Out {
			if e == nil || e.Callee == nil {
				continue
			}
			calleeID, ok := ssaFnID(e.Callee.Func, objToID)
			if !ok || callerID == calleeID {
				continue
			}
			key := [2]int{callerID, calleeID}
			if seen[key] {
				continue
			}
			if len(out.Edges) >= maxEdges {
				out.Truncated = true
				sortEdges(out.Edges)
				return
			}
			seen[key] = true
			out.Edges = append(out.Edges, edge{CallerID: callerID, CalleeID: calleeID})
		}
	}
	sortEdges(out.Edges)
}

func sortEdges(e []edge) {
	sort.Slice(e, func(i, j int) bool {
		if e[i].CallerID != e[j].CallerID {
			return e[i].CallerID < e[j].CallerID
		}
		return e[i].CalleeID < e[j].CalleeID
	})
}

func ssaFnID(fn *ssa.Function, m map[types.Object]int) (int, bool) {
	if fn == nil {
		return 0, false
	}
	obj := fn.Object()
	if obj == nil {
		return 0, false
	}
	id, ok := m[obj]
	return id, ok
}

// recvTypeName extracts the bare receiver type name from a method receiver
// expression (`T`, `*T`, `T[X]`, `*T[X]`).
func recvTypeName(expr ast.Expr) string {
	switch e := expr.(type) {
	case *ast.StarExpr:
		return recvTypeName(e.X)
	case *ast.Ident:
		return e.Name
	case *ast.IndexExpr: // generic receiver T[X]
		return recvTypeName(e.X)
	case *ast.IndexListExpr:
		return recvTypeName(e.X)
	}
	return ""
}

// relPath renders an absolute source path repo-relative with slashes; on failure
// (or escape) it falls back to the base name so no absolute path leaks.
func relPath(root, file string) string {
	rel, err := filepath.Rel(root, file)
	if err != nil || rel == "" {
		return filepath.Base(file)
	}
	return filepath.ToSlash(rel)
}

// pkgDir returns the repo-relative directory of a package (from its first file).
func pkgDir(root string, p *packages.Package) string {
	files := p.GoFiles
	if len(files) == 0 {
		files = p.CompiledGoFiles
	}
	if len(files) == 0 {
		return ""
	}
	return filepath.ToSlash(filepath.Dir(relPath(root, files[0])))
}

func bound(s string) string {
	if len(s) <= maxStr {
		return s
	}
	// Truncate on a rune boundary.
	r := []rune(s)
	if len(r) > maxStr {
		r = r[:maxStr]
	}
	return string(r)
}
