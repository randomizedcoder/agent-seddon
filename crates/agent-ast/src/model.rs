//! `SymbolModel` — a symbol table + interface-implementation index, shared by the
//! symbol-shaped verbs (`find_symbol`, `implementations`, `interface_of`). The Go
//! engine answers those from its call graph; the SCIP engine builds one of these by
//! ingesting `.scip` indexes. Kept free of any IO so ingestion + queries are
//! unit-testable from an in-memory `scip::types::Index`.

use agent_core::{Symbol, SymbolKind, SymbolQuery, SymbolRef};
use std::collections::HashMap;

const MAX_SYMBOLS: usize = 200_000;
const MAX_RESULT: usize = 500;
const MAX_STR: usize = 256;

/// A cross-language symbol table with an interface-implementation index. Ids are
/// assigned densely as symbols are first seen (a SCIP symbol string → one id).
#[derive(Debug, Default)]
pub struct SymbolModel {
    symbols: Vec<Symbol>,
    /// SCIP symbol string → our dense id.
    scip_id: HashMap<String, u32>,
    /// symbol name → ids.
    by_name: HashMap<String, Vec<u32>>,
    /// interface id → concrete type ids that implement it.
    impls_of: HashMap<u32, Vec<u32>>,
    /// concrete type id → interface ids it implements.
    ifaces_of: HashMap<u32, Vec<u32>>,
    languages: Vec<String>,
}

impl SymbolModel {
    pub fn symbol_count(&self) -> usize {
        self.symbols.len()
    }

    pub fn set_languages(&mut self, langs: Vec<String>) {
        self.languages = langs;
    }

    /// Get-or-create the id for a SCIP symbol string. When `def` is `Some`, this is a
    /// *definition* site, so its name/kind/file/line are recorded (authoritative over
    /// a placeholder created for a relationship target).
    fn intern(&mut self, scip_symbol: &str, def: Option<SymbolDef>) -> Option<u32> {
        if let Some(&id) = self.scip_id.get(scip_symbol) {
            if let Some(d) = def {
                // Upgrade a placeholder (created as a relationship target) with real
                // definition info.
                let idx = id as usize;
                if self.symbols[idx].file.is_empty() {
                    self.symbols[idx].file = d.file;
                    self.symbols[idx].line = d.line;
                }
                if self.symbols[idx].kind == SymbolKind::Unknown {
                    self.symbols[idx].kind = d.kind;
                }
            }
            return Some(id);
        }
        if self.symbols.len() >= MAX_SYMBOLS {
            return None;
        }
        let id = self.symbols.len() as u32;
        let (parsed_name, package) = symbol_name_package(scip_symbol);
        let d = def.unwrap_or_default();
        // Prefer the indexer's display name, then the parsed descriptor, then the raw
        // symbol string.
        let name = if !d.name.is_empty() {
            d.name.clone()
        } else if !parsed_name.is_empty() {
            parsed_name
        } else {
            scip_symbol.to_string()
        };
        let sym = Symbol {
            id,
            kind: d.kind,
            name: bound(&name),
            recv: String::new(),
            package: bound(&package),
            file: d.file,
            line: d.line,
            exported: true, // SCIP symbols are cross-file references ⇒ treat as public
        };
        self.by_name.entry(sym.name.clone()).or_default().push(id);
        self.scip_id.insert(scip_symbol.to_string(), id);
        self.symbols.push(sym);
        Some(id)
    }

    /// Record that `concrete` implements `iface` (both already interned).
    fn add_impl(&mut self, iface: u32, concrete: u32) {
        if iface == concrete {
            return;
        }
        let a = self.impls_of.entry(iface).or_default();
        if !a.contains(&concrete) {
            a.push(concrete);
        }
        let b = self.ifaces_of.entry(concrete).or_default();
        if !b.contains(&iface) {
            b.push(iface);
        }
    }

    fn sym(&self, id: u32) -> Option<&Symbol> {
        self.symbols.get(id as usize)
    }

    fn resolve(&self, r: &SymbolRef) -> Vec<u32> {
        if let Some(id) = r.id {
            return if (id as usize) < self.symbols.len() {
                vec![id]
            } else {
                Vec::new()
            };
        }
        self.by_name
            .get(&r.name)
            .map(|ids| {
                ids.iter()
                    .copied()
                    .filter(|id| {
                        r.package.as_deref().is_none_or(|p| {
                            self.sym(*id)
                                .is_some_and(|s| s.package.ends_with(p) || s.package == p)
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn symbols_for(&self, ids: impl IntoIterator<Item = u32>) -> Vec<Symbol> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for id in ids {
            if seen.insert(id) {
                if let Some(s) = self.sym(id) {
                    out.push(s.clone());
                    if out.len() >= MAX_RESULT {
                        break;
                    }
                }
            }
        }
        out
    }

    pub fn find_symbol(&self, q: &SymbolQuery) -> Vec<Symbol> {
        let needle = q.name.to_lowercase();
        let limit = q.limit.min(MAX_RESULT);
        let mut out = Vec::new();
        for s in &self.symbols {
            if q.kind.is_some_and(|k| k != s.kind) {
                continue;
            }
            if q.package
                .as_deref()
                .is_some_and(|p| !(s.package.ends_with(p) || s.package == p))
            {
                continue;
            }
            let hay = s.name.to_lowercase();
            let hit = if q.exact {
                hay == needle
            } else {
                hay.contains(&needle)
            };
            if hit {
                out.push(s.clone());
                if out.len() >= limit {
                    break;
                }
            }
        }
        out
    }

    pub fn implementations(&self, iface: &SymbolRef) -> Vec<Symbol> {
        let ids = self
            .resolve(iface)
            .into_iter()
            .flat_map(|id| self.impls_of.get(&id).into_iter().flatten().copied());
        self.symbols_for(ids)
    }

    pub fn interface_of(&self, ty: &SymbolRef) -> Vec<Symbol> {
        let ids = self
            .resolve(ty)
            .into_iter()
            .flat_map(|id| self.ifaces_of.get(&id).into_iter().flatten().copied());
        self.symbols_for(ids)
    }
}

/// A definition's attributes, recorded when a symbol is interned at its def site.
#[derive(Debug, Default, Clone)]
struct SymbolDef {
    name: String,
    kind: SymbolKind,
    file: String,
    line: u32,
}

// --- SCIP ingestion --------------------------------------------------------

impl SymbolModel {
    /// Decode a raw `.scip` protobuf and fold it in. Returns `false` on a decode
    /// failure (fail-soft). Lets callers/tests ingest bytes without depending on the
    /// `scip`/`protobuf` crates themselves.
    pub fn ingest_scip_bytes(&mut self, bytes: &[u8], root: &std::path::Path, lang: &str) -> bool {
        use protobuf::Message;
        match scip::types::Index::parse_from_bytes(bytes) {
            Ok(index) => {
                self.ingest_scip(&index, root, lang);
                true
            }
            Err(_) => false,
        }
    }

    /// Fold a decoded SCIP `Index` into the model: each `SymbolInformation` becomes a
    /// symbol (defined in the document it is listed under, positioned from a
    /// Definition-role occurrence), and each `Relationship { is_implementation }`
    /// records "this symbol implements the related interface".
    pub fn ingest_scip(&mut self, index: &scip::types::Index, root: &std::path::Path, _lang: &str) {
        use scip::types::SymbolRole;
        let def_role = SymbolRole::Definition as i32;

        for doc in &index.documents {
            // Confine the document path; skip a document that escapes the repo.
            let file = match agent_core::confine(root, &doc.relative_path) {
                Ok(_) => doc.relative_path.clone(),
                Err(_) => continue,
            };

            // Definition line per symbol, from occurrences carrying the Definition
            // role (range = [startLine, startCol, endCol] or [sl, sc, el, ec]).
            let mut def_line: HashMap<&str, u32> = HashMap::new();
            for occ in &doc.occurrences {
                if occ.symbol_roles & def_role != 0 {
                    if let Some(&sl) = occ.range.first() {
                        def_line.insert(occ.symbol.as_str(), (sl.max(0) as u32) + 1);
                    }
                }
            }

            for si in &doc.symbols {
                // Skip empty + function-local symbols (`local …`) — locals are noise
                // for whole-repo symbol/implementation queries.
                if si.symbol.is_empty() || si.symbol.starts_with("local ") {
                    continue;
                }
                let kind = crate::scip::kind_from_scip(si.kind.enum_value_or_default());
                let name = if si.display_name.is_empty() {
                    String::new()
                } else {
                    si.display_name.clone()
                };
                let line = def_line.get(si.symbol.as_str()).copied().unwrap_or(0);
                let Some(id) = self.intern(
                    &si.symbol,
                    Some(SymbolDef {
                        name,
                        kind,
                        file: file.clone(),
                        line,
                    }),
                ) else {
                    return; // symbol cap hit
                };

                for rel in &si.relationships {
                    if rel.is_implementation && !rel.symbol.is_empty() {
                        // `si` implements `rel.symbol` ⇒ rel.symbol is the interface.
                        if let Some(iface) = self.intern(&rel.symbol, None) {
                            self.add_impl(iface, id);
                        }
                    }
                }
            }
        }
    }
}

/// Extract a human name + package from a SCIP symbol string via the descriptor
/// grammar. Returns empty strings when the symbol doesn't parse (the caller then
/// falls back to the display name or the raw string). Never fails.
fn symbol_name_package(scip_symbol: &str) -> (String, String) {
    match scip::symbol::parse_symbol(scip_symbol) {
        Ok(sym) => {
            let name = sym
                .descriptors
                .last()
                .map(|d| d.name.clone())
                .unwrap_or_default();
            let package = sym
                .package
                .as_ref()
                .map(|p| p.name.clone())
                .unwrap_or_default();
            (name, package)
        }
        Err(_) => (String::new(), String::new()),
    }
}

fn bound(s: &str) -> String {
    if s.len() <= MAX_STR {
        s.to_string()
    } else {
        s.chars().take(MAX_STR).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scip::types::symbol_information::Kind;
    use scip::types::{Document, Index, Occurrence, Relationship, SymbolInformation, SymbolRole};

    fn root() -> std::path::PathBuf {
        agent_testkit::tempdir()
    }

    fn si(symbol: &str, name: &str, kind: Kind) -> SymbolInformation {
        let mut s = SymbolInformation::new();
        s.symbol = symbol.into();
        s.display_name = name.into();
        s.kind = kind.into();
        s
    }

    /// An index with an interface `Greeter` and two implementers (`Polite`, `Loud`),
    /// each carrying an `is_implementation` relationship to the interface.
    fn greeter_index() -> Index {
        let iface_sym = "scip-go . . `greeter`/Greeter#";
        let mut doc = Document::new();
        doc.relative_path = "greeter/greeter.go".into();
        doc.language = "go".into();

        let iface = si(iface_sym, "Greeter", Kind::Interface);
        let mut polite = si("scip-go . . `greeter`/Polite#", "Polite", Kind::Struct);
        let mut loud = si("scip-go . . `greeter`/Loud#", "Loud", Kind::Struct);
        for c in [&mut polite, &mut loud] {
            let mut rel = Relationship::new();
            rel.symbol = iface_sym.into();
            rel.is_implementation = true;
            c.relationships = vec![rel];
        }
        let polite_sym = polite.symbol.clone();
        doc.symbols = vec![iface, polite, loud];

        let mut occ = Occurrence::new();
        occ.symbol = polite_sym;
        occ.symbol_roles = SymbolRole::Definition as i32;
        occ.range = vec![10, 0, 6];
        doc.occurrences = vec![occ];

        let mut index = Index::new();
        index.documents = vec![doc];
        index
    }

    fn ingested() -> SymbolModel {
        let mut m = SymbolModel::default();
        m.ingest_scip(&greeter_index(), &root(), "go");
        m
    }

    fn names(syms: &[Symbol]) -> Vec<String> {
        let mut v: Vec<String> = syms.iter().map(|s| s.name.clone()).collect();
        v.sort();
        v
    }

    #[test]
    fn positive_implementations_across_scip_relationships() {
        let m = ingested();
        let impls = m.implementations(&SymbolRef::name("Greeter"));
        assert_eq!(
            names(&impls),
            vec!["Loud".to_string(), "Polite".to_string()]
        );
    }

    #[test]
    fn positive_interface_of_inverts_the_relation() {
        let m = ingested();
        let ifaces = m.interface_of(&SymbolRef::name("Polite"));
        assert_eq!(names(&ifaces), vec!["Greeter".to_string()]);
    }

    #[test]
    fn positive_find_symbol_and_definition_line() {
        let m = ingested();
        let q = SymbolQuery {
            name: "Pol".into(),
            kind: None,
            package: None,
            exact: false,
            limit: 10,
        };
        let hits = m.find_symbol(&q);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "Polite");
        assert_eq!(hits[0].file, "greeter/greeter.go");
        assert_eq!(hits[0].line, 11, "def occurrence line 10 (0-based) → 11");
    }

    #[test]
    fn negative_unknown_symbol_is_empty() {
        let m = ingested();
        assert!(m.implementations(&SymbolRef::name("Nope")).is_empty());
        assert!(m.interface_of(&SymbolRef::name("Nope")).is_empty());
    }

    #[test]
    fn adversarial_document_path_escape_is_dropped() {
        let mut index = greeter_index();
        index.documents[0].relative_path = "../../etc/passwd".into();
        let mut m = SymbolModel::default();
        m.ingest_scip(&index, &root(), "go");
        assert_eq!(m.symbol_count(), 0, "escaping document dropped whole");
    }

    #[test]
    fn corner_empty_index_is_queryable() {
        let mut m = SymbolModel::default();
        m.ingest_scip(&Index::new(), &root(), "go");
        assert_eq!(m.symbol_count(), 0);
        assert!(m
            .find_symbol(&SymbolQuery {
                name: "x".into(),
                kind: None,
                package: None,
                exact: false,
                limit: 5
            })
            .is_empty());
    }
}
