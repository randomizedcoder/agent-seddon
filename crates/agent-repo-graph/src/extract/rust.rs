use super::{
    cargo::{relative, Catalog, Package},
    clipped, hash, screened, Context,
};
use crate::{EdgeKind, GraphBuilder, NodeKey, NodeKind, NodeSpec, RepoGraphResult};
use proc_macro2::{Span, TokenStream, TokenTree};
use quote::ToTokens;
use serde_json::{json, Value};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use syn::{parse::Parser, punctuated::Punctuated, spanned::Spanned, Attribute, Item, Meta, Token};

struct Pending {
    kind: EdgeKind,
    src: NodeKey,
    path: String,
}
struct Function {
    key: NodeKey,
    module: String,
    name: String,
    test: bool,
}

struct Visitor<'a, 'b> {
    cx: &'a mut Context<'b>,
    out: &'a mut GraphBuilder,
    pkg: &'a Package,
    definitions: BTreeMap<String, Vec<NodeKey>>,
    pending: Vec<Pending>,
    functions: Vec<Function>,
    active: BTreeSet<String>,
    workspace_crates: BTreeSet<String>,
    impl_keys: BTreeSet<(NodeKey, Vec<String>)>,
    aliases: BTreeMap<String, String>,
}

#[inline(never)] // Stable callgrind entry point on the scoped parser thread.
pub(super) fn emit(
    cx: &mut Context<'_>,
    out: &mut GraphBuilder,
    catalog: &Catalog,
) -> RepoGraphResult<()> {
    // Resolve after visiting every workspace crate so cross-crate imports are order independent.
    let mut definitions = BTreeMap::<String, Vec<NodeKey>>::new();
    let mut pending = Vec::new();
    let mut aliases = BTreeMap::new();
    for pkg in &catalog.packages {
        let mut visitor = Visitor {
            cx,
            out,
            pkg,
            definitions: BTreeMap::new(),
            pending: Vec::new(),
            functions: Vec::new(),
            active: BTreeSet::new(),
            workspace_crates: catalog.packages.iter().map(|p| p.name.clone()).collect(),
            impl_keys: BTreeSet::new(),
            aliases: BTreeMap::new(),
        };
        for (file, module) in &pkg.roots {
            let parent = NodeKey::rust_crate(&pkg.name)?;
            let dir = Path::new(file).parent().unwrap_or(Path::new(""));
            visitor.file(file, module, dir, &parent, &[], 0)?;
        }
        visitor.test_edges();
        for (path, keys) in visitor.definitions {
            definitions.entry(path).or_default().extend(keys);
        }
        pending.extend(visitor.pending);
        aliases.extend(visitor.aliases);
    }
    for pending in pending {
        cx.check()?;
        let path = resolve_aliases(&pending.path, &aliases);
        if let Some(keys) = path.as_ref().and_then(|p| definitions.get(p)) {
            for key in keys.iter().take(4) {
                out.edge(
                    pending.kind,
                    &pending.src,
                    key,
                    1.0,
                    json!({"resolved": true}),
                );
            }
        }
        // Unresolved relationships are stored as bounded source attributes, never fabricated edges.
    }
    Ok(())
}

impl Visitor<'_, '_> {
    fn file(
        &mut self,
        file: &str,
        module: &str,
        dir: &Path,
        parent: &NodeKey,
        cfg: &[String],
        depth: usize,
    ) -> RepoGraphResult<()> {
        self.cx.check()?;
        if depth > 128 || !self.active.insert(file.to_owned()) {
            self.cx.skip();
            return Ok(());
        }
        let Some(text) = self.cx.text(file)? else {
            self.active.remove(file);
            return Ok(());
        };
        match syn::parse_file(&text) {
            Ok(ast) => {
                self.cx.check()?;
                let mut file_cfg = cfg.to_vec();
                file_cfg.extend(
                    ast.attrs
                        .iter()
                        .filter(|a| a.path().is_ident("cfg"))
                        .map(|a| clipped(&a.meta.to_token_stream().to_string(), 256)),
                );
                if file_cfg.len() > 16 {
                    self.cx.report.truncated = true;
                }
                file_cfg.truncate(16);
                let key = NodeKey::rust_mod(&self.pkg.name, module)?;
                let attrs = json!({"resolved_calls": false, "macro_expansion": false,
                    "external_imports": external_imports(&ast.items, &self.workspace_crates)});
                let spec = NodeSpec::new(
                    key,
                    if module.is_empty() {
                        &self.pkg.name
                    } else {
                        module
                    },
                )
                .with_file(file)
                .with_cfg(file_cfg.clone())
                .with_attrs(attrs);
                if let Some(key) = self.cx.node(self.out, spec) {
                    self.edge(EdgeKind::Contains, parent, &key);
                    self.define(module, "", &key);
                    self.items(&ast.items, file, module, dir, &key, &file_cfg, depth)?;
                }
            }
            Err(_) => self.cx.parse_error(),
        }
        self.active.remove(file);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn items(
        &mut self,
        items: &[Item],
        file: &str,
        module: &str,
        dir: &Path,
        parent: &NodeKey,
        inherited: &[String],
        depth: usize,
    ) -> RepoGraphResult<()> {
        if depth > 128 {
            self.cx.skip();
            return Ok(());
        }
        let items = coalesced_impls(items);
        for item in &items {
            let item = item.as_ref();
            self.cx.check()?;
            let attrs = attributes(item);
            let mut cfg = inherited.to_vec();
            cfg.extend(
                attrs
                    .iter()
                    .filter(|a| a.path().is_ident("cfg"))
                    .map(|a| clipped(&a.meta.to_token_stream().to_string(), 256)),
            );
            if cfg.len() > 16 {
                self.cx.report.truncated = true;
            }
            cfg.truncate(16);
            match item {
                Item::Fn(f) => {
                    let name = ident(&f.sig.ident);
                    let test = is_test(attrs);
                    let key = if test {
                        NodeKey::rust_test(&self.pkg.name, module, &name, None)
                    } else {
                        NodeKey::rust_item(NodeKind::Fn, &self.pkg.name, module, &name)
                    };
                    let cases = named_cases(attrs);
                    let mut data = fn_attrs(&f.sig);
                    data["cases"] = json!(cases.len());
                    data["test_class"] = json!(test_class(&name));
                    if let Some(key) = self.item(
                        key,
                        &name,
                        file,
                        module,
                        f.span(),
                        &f.sig,
                        item,
                        attrs,
                        &cfg,
                        &f.vis,
                        data,
                        parent,
                    ) {
                        self.functions.push(Function {
                            key: key.clone(),
                            name: name.clone(),
                            module: module.into(),
                            test,
                        });
                        for case in cases.into_iter().collect::<BTreeSet<_>>() {
                            let case_key =
                                NodeKey::rust_test(&self.pkg.name, module, &name, Some(&case));
                            self.item(
                                case_key,
                                &case,
                                file,
                                module,
                                f.span(),
                                &f.sig,
                                item,
                                attrs,
                                &cfg,
                                &f.vis,
                                json!({"test_class": test_class(&case)}),
                                &key,
                            );
                        }
                    }
                }
                Item::Mod(m) => {
                    let name = ident(&m.ident);
                    let child = join(module, &name);
                    if let Some((_, items)) = &m.content {
                        if let Some(key) = self.item(
                            NodeKey::rust_mod(&self.pkg.name, &child),
                            &name,
                            file,
                            module,
                            m.span(),
                            &m.ident,
                            item,
                            attrs,
                            &cfg,
                            &m.vis,
                            json!({}),
                            parent,
                        ) {
                            self.items(
                                items,
                                file,
                                &child,
                                &dir.join(&name),
                                &key,
                                &cfg,
                                depth + 1,
                            )?;
                        }
                    } else {
                        let explicit = attrs
                            .iter()
                            .find(|a| a.path().is_ident("path"))
                            .and_then(attr_string);
                        let paths = if let Some(path) = explicit {
                            vec![dir.join(path)]
                        } else {
                            vec![
                                dir.join(format!("{name}.rs")),
                                dir.join(&name).join("mod.rs"),
                            ]
                        };
                        let target = paths
                            .iter()
                            .filter_map(|p| p.to_str())
                            .filter_map(|p| relative(self.cx, "", p))
                            .find(|p| self.cx.files.contains(p));
                        if let Some(target) = target {
                            let next_dir = if target.ends_with("/mod.rs") {
                                Path::new(&target).parent().unwrap().to_owned()
                            } else {
                                Path::new(&target).with_extension("")
                            };
                            self.file(&target, &child, &next_dir, parent, &cfg, depth + 1)?;
                        } else {
                            self.item(
                                NodeKey::rust_mod(&self.pkg.name, &child),
                                &name,
                                file,
                                module,
                                m.span(),
                                &m.ident,
                                item,
                                attrs,
                                &cfg,
                                &m.vis,
                                json!({"unresolved": true}),
                                parent,
                            );
                            self.cx.report.truncated = true;
                        }
                    }
                }
                Item::Impl(i) => {
                    let self_name = type_name(&i.self_ty);
                    let trait_path = i.trait_.as_ref().map(|(_, path, _)| {
                        path.segments
                            .iter()
                            .map(|s| ident(&s.ident))
                            .collect::<Vec<_>>()
                            .join("::")
                    });
                    let normalized = trait_path.as_deref().map(|p| {
                        p.rsplit("::")
                            .take(2)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect::<Vec<_>>()
                            .join("::")
                    });
                    let mut key = NodeKey::rust_impl(
                        &self.pkg.name,
                        module,
                        &self_name,
                        normalized.as_deref(),
                    );
                    // Several blocks for the same type are coalesced above. Distinct
                    // generic specializations with the same erased key need an identity suffix.
                    if let Ok(base) = &key {
                        let mut sorted_cfg = cfg.clone();
                        sorted_cfg.sort();
                        if !self.impl_keys.insert((base.clone(), sorted_cfg)) {
                            key = NodeKey::parse(&format!(
                                "{}@{}",
                                base.as_str(),
                                &hash(impl_header(i))[..8]
                            ));
                        }
                    }
                    if let Some(key) = self.item(key, &self_name, file, module, i.span(), &i.self_ty, item, attrs, &cfg, &syn::Visibility::Inherited,
                        json!({"trait_path": trait_path.as_deref().map(|p| clipped(p, 256)), "self_path": clipped(&i.self_ty.to_token_stream().to_string(), 256)}), parent) {
                        self.pending.push(Pending { kind: EdgeKind::ImplFor, src: key.clone(), path: self.resolve_path(module, &type_path(&i.self_ty)) });
                        if let Some(path) = trait_path { self.pending.push(Pending { kind: EdgeKind::Implements, src: key.clone(), path: self.resolve_path(module, &path) }); }
                        for impl_item in &i.items {
                            if let syn::ImplItem::Fn(f) = impl_item {
                                let name = ident(&f.sig.ident);
                                let mut method_cfg = cfg.clone();
                                method_cfg.extend(f.attrs.iter().filter(|a| a.path().is_ident("cfg")).map(|a| clipped(&a.meta.to_token_stream().to_string(), 256)));
                                let mut method = NodeKey::rust_method(&self.pkg.name, module, &self_name, normalized.as_deref(), &name);
                                if let Some((_, suffix)) = key.as_str().rsplit_once('@') {
                                    method = method.and_then(|k| NodeKey::parse(&format!("{}@{suffix}", k.as_str())));
                                }
                                if let Some(key) = self.item(method, &name, file, module, f.span(), &f.sig, f, &f.attrs, &method_cfg, &f.vis, fn_attrs(&f.sig), &key) {
                                    self.functions.push(Function { key, name, module: module.into(), test: false });
                                }
                            }
                        }
                    }
                }
                Item::Use(u) => {
                    let mut paths = Vec::new();
                    use_paths(&u.tree, "", &mut paths);
                    for path in paths.into_iter().take(64) {
                        self.pending.push(Pending {
                            kind: EdgeKind::Imports,
                            src: parent.clone(),
                            path: self.resolve_path(module, &path),
                        });
                    }
                    let mut uses = Vec::new();
                    use_aliases(&u.tree, "", &mut uses);
                    for (alias, path) in uses {
                        self.aliases.insert(
                            join(&join(&self.pkg.name, module), &alias),
                            self.resolve_path(module, &path),
                        );
                    }
                }
                _ => {
                    let simple = match item {
                        Item::Struct(s) => Some((
                            NodeKind::Struct,
                            &s.ident,
                            &s.vis,
                            json!({"fields": s.fields.len()}),
                        )),
                        Item::Enum(e) => Some((
                            NodeKind::Enum,
                            &e.ident,
                            &e.vis,
                            json!({"variants": e.variants.len()}),
                        )),
                        Item::Trait(t) => Some((
                            NodeKind::Trait,
                            &t.ident,
                            &t.vis,
                            json!({"async_trait": attrs.iter().any(|a| a.path().is_ident("async_trait")), "is_seam": attrs.iter().any(|a| a.path().is_ident("async_trait"))}),
                        )),
                        Item::Type(t) => Some((NodeKind::Type, &t.ident, &t.vis, json!({}))),
                        Item::Const(c) if c.ident != "_" => {
                            Some((NodeKind::Const, &c.ident, &c.vis, json!({})))
                        }
                        Item::Static(s) => Some((NodeKind::Static, &s.ident, &s.vis, json!({}))),
                        _ => None,
                    };
                    if let Some((kind, name, vis, data)) = simple {
                        let name = ident(name);
                        let key = NodeKey::rust_item(kind, &self.pkg.name, module, &name);
                        // For non-functions strip outer attributes from the signature stream.
                        self.item(
                            key,
                            &name,
                            file,
                            module,
                            item.span(),
                            &without_outer_attrs(item.to_token_stream()),
                            item,
                            attrs,
                            &cfg,
                            vis,
                            data,
                            parent,
                        );
                    } else if let Item::Macro(m) = item {
                        if let Some(name) = &m.ident {
                            let name = ident(name);
                            self.item(
                                NodeKey::rust_item(NodeKind::Macro, &self.pkg.name, module, &name),
                                &name,
                                file,
                                module,
                                m.span(),
                                &m.mac.path,
                                item,
                                attrs,
                                &cfg,
                                &syn::Visibility::Inherited,
                                json!({"expanded": false}),
                                parent,
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn item(
        &mut self,
        key: RepoGraphResult<NodeKey>,
        name: &str,
        file: &str,
        module: &str,
        span: Span,
        signature: &impl ToTokens,
        body: &impl ToTokens,
        attrs: &[Attribute],
        cfg: &[String],
        vis: &syn::Visibility,
        mut data: Value,
        parent: &NodeKey,
    ) -> Option<NodeKey> {
        let key = match key {
            Ok(k) => k,
            Err(_) => {
                self.cx.skip();
                return None;
            }
        };
        let doc = attrs
            .iter()
            .filter(|a| a.path().is_ident("doc"))
            .filter_map(attr_string)
            .collect::<Vec<_>>()
            .join("\n");
        let paragraph = doc.trim().split("\n\n").next().unwrap_or("");
        let doc = screened(paragraph, 512);
        if !doc.is_empty() {
            data["doc"] = json!(doc);
        }
        if !cfg.is_empty() {
            data["cfg"] = json!(cfg);
        }
        if let Some(ignore) = attrs.iter().find(|a| a.path().is_ident("ignore")) {
            data["ignored"] = json!(true);
            data["ignore_reason"] = json!(screened(&attr_string(ignore).unwrap_or_default(), 160));
        }
        let spec = NodeSpec::new(key, name)
            .with_file(file)
            .with_qualifier(join(&self.pkg.name, module))
            .with_lines(span.start().line as i32, span.end().line as i32)
            .with_sig(hash(strip_docs(signature.to_token_stream()).to_string()))
            .with_body(hash(strip_docs(body.to_token_stream()).to_string()))
            .with_cfg(cfg.to_vec())
            .with_exported(!matches!(vis, syn::Visibility::Inherited))
            .with_attrs(data);
        let key = self.cx.node(self.out, spec)?;
        self.edge(EdgeKind::Contains, parent, &key);
        if let Ok(file) = NodeKey::file(file) {
            self.edge(EdgeKind::DefinedIn, &key, &file);
        }
        if !matches!(
            key.kind(),
            NodeKind::Impl | NodeKind::Method | NodeKind::Test
        ) {
            self.define(module, name, &key);
        }
        let mut features = BTreeSet::new();
        for attr in attrs.iter().filter(|a| a.path().is_ident("cfg")) {
            if let Meta::List(list) = &attr.meta {
                if let Ok(meta) = syn::parse2::<Meta>(list.tokens.clone()) {
                    cfg_features(&meta, &mut features);
                }
            }
        }
        // Include inherited positive feature gates without interpreting negation as a gate.
        for cfg in cfg {
            if let Ok(Meta::List(list)) = syn::parse_str::<Meta>(cfg) {
                if let Ok(meta) = syn::parse2::<Meta>(list.tokens) {
                    cfg_features(&meta, &mut features);
                }
            }
        }
        for feature in features {
            if let Ok(feature) = NodeKey::rust_feature(&self.pkg.name, &feature) {
                self.edge(EdgeKind::GatedBy, &key, &feature);
            }
        }
        Some(key)
    }

    fn define(&mut self, module: &str, name: &str, key: &NodeKey) {
        self.definitions
            .entry(join(&join(&self.pkg.name, module), name))
            .or_default()
            .push(key.clone());
    }

    fn edge(&mut self, kind: EdgeKind, from: &NodeKey, to: &NodeKey) {
        self.out.edge(kind, from, to, 1.0, json!({}));
    }

    fn resolve_path(&self, module: &str, path: &str) -> String {
        let mut parts: Vec<_> = path.split("::").filter(|p| !p.is_empty()).collect();
        let mut base = vec![self.pkg.name.as_str()];
        match parts.first().copied() {
            Some("crate") => {
                parts.remove(0);
            }
            Some("self" | "super") => {
                base.extend(module.split("::").filter(|p| !p.is_empty()));
                if parts.first() == Some(&"self") {
                    parts.remove(0);
                }
                while parts.first() == Some(&"super") {
                    parts.remove(0);
                    if base.len() > 1 {
                        base.pop();
                    }
                }
            }
            Some(first) if self.workspace_crates.contains(first) => return parts.join("::"),
            Some(first) if parts.len() > 1 && matches!(first, "std" | "core" | "alloc") => {
                return parts.join("::")
            }
            _ => base.extend(module.split("::").filter(|p| !p.is_empty())),
        }
        base.extend(parts);
        base.join("::")
    }

    fn test_edges(&mut self) {
        for test in self.functions.iter().filter(|f| f.test) {
            let module = if test.module == "tests" {
                ""
            } else {
                test.module.strip_suffix("::tests").unwrap_or(&test.module)
            };
            for target in self.functions.iter().filter(|f| !f.test) {
                if target.module == module
                    && (test.name.starts_with(&format!("{}_", target.name))
                        || test.name.ends_with(&format!("_{}", target.name)))
                {
                    self.out.edge(
                        EdgeKind::Tests,
                        &test.key,
                        &target.key,
                        1.0,
                        json!({"via": "name"}),
                    );
                }
            }
        }
    }
}

fn impl_header(item: &syn::ItemImpl) -> String {
    let mut header = item.generics.to_token_stream();
    item.generics.where_clause.to_tokens(&mut header);
    item.self_ty.to_tokens(&mut header);
    if let Some((negative, path, _)) = &item.trait_ {
        negative.to_tokens(&mut header);
        path.to_tokens(&mut header);
    }
    for attr in item.attrs.iter().filter(|a| a.path().is_ident("cfg")) {
        attr.to_tokens(&mut header);
    }
    header.to_string()
}

fn coalesced_impls(items: &[Item]) -> Vec<Cow<'_, Item>> {
    let mut positions = BTreeMap::new();
    let mut result: Vec<Cow<'_, Item>> = Vec::new();
    for item in items {
        if let Item::Impl(implementation) = item {
            let header = impl_header(implementation);
            if let Some(&index) = positions.get(&header) {
                let existing: &mut Cow<'_, Item> = &mut result[index];
                if let Item::Impl(existing) = existing.to_mut() {
                    existing.items.extend(implementation.items.clone());
                }
                continue;
            }
            positions.insert(header, result.len());
        }
        result.push(Cow::Borrowed(item));
    }
    result
}

fn join(a: &str, b: &str) -> String {
    if a.is_empty() {
        b.into()
    } else if b.is_empty() {
        a.into()
    } else {
        format!("{a}::{b}")
    }
}
fn use_paths(tree: &syn::UseTree, prefix: &str, paths: &mut Vec<String>) {
    if paths.len() >= 64 {
        return;
    }
    match tree {
        syn::UseTree::Path(p) => use_paths(&p.tree, &join(prefix, &ident(&p.ident)), paths),
        syn::UseTree::Name(n) => paths.push(if n.ident == "self" {
            prefix.into()
        } else {
            join(prefix, &ident(&n.ident))
        }),
        syn::UseTree::Rename(n) => paths.push(join(prefix, &ident(&n.ident))),
        syn::UseTree::Group(g) => {
            for item in &g.items {
                use_paths(item, prefix, paths);
            }
        }
        syn::UseTree::Glob(_) => {}
    }
}
fn use_aliases(tree: &syn::UseTree, prefix: &str, aliases: &mut Vec<(String, String)>) {
    if aliases.len() >= 64 {
        return;
    }
    match tree {
        syn::UseTree::Path(p) => use_aliases(&p.tree, &join(prefix, &ident(&p.ident)), aliases),
        syn::UseTree::Name(n) if n.ident == "self" => {
            aliases.push((
                prefix.rsplit("::").next().unwrap_or("").into(),
                prefix.into(),
            ));
        }
        syn::UseTree::Name(n) => aliases.push((ident(&n.ident), join(prefix, &ident(&n.ident)))),
        syn::UseTree::Rename(n) => aliases.push((ident(&n.rename), join(prefix, &ident(&n.ident)))),
        syn::UseTree::Group(g) => {
            for item in &g.items {
                use_aliases(item, prefix, aliases);
            }
        }
        _ => {}
    }
}
fn resolve_aliases(path: &str, aliases: &BTreeMap<String, String>) -> Option<String> {
    let mut path = path.to_owned();
    let mut seen = BTreeSet::new();
    for _ in 0..8 {
        if !seen.insert(path.clone()) {
            return None;
        }
        let mut prefix = path.as_str();
        let mut replacement = None;
        loop {
            if let Some(target) = aliases.get(prefix) {
                if target != prefix {
                    replacement = Some(format!("{target}{}", &path[prefix.len()..]));
                }
                break;
            }
            let Some((parent, _)) = prefix.rsplit_once("::") else {
                break;
            };
            prefix = parent;
        }
        match replacement {
            Some(next) => path = next,
            None => return Some(path),
        }
    }
    None
}
fn external_imports(items: &[Item], crates: &BTreeSet<String>) -> Vec<Value> {
    let mut paths = Vec::new();
    for item in items {
        if let Item::Use(u) = item {
            use_paths(&u.tree, "", &mut paths);
        }
    }
    paths
        .into_iter()
        .filter(|p| {
            p.split("::").next().is_some_and(|first| {
                !crates.contains(first) && !matches!(first, "crate" | "self" | "super")
            })
        })
        .take(8)
        .map(|p| json!({"path": screened(&p, 128), "resolved": false}))
        .collect()
}
fn ident(i: &syn::Ident) -> String {
    i.to_string().trim_start_matches("r#").to_owned()
}
fn type_path(t: &syn::Type) -> String {
    if let syn::Type::Path(p) = t {
        p.path
            .segments
            .iter()
            .map(|s| ident(&s.ident))
            .collect::<Vec<_>>()
            .join("::")
    } else {
        String::new()
    }
}
fn type_name(t: &syn::Type) -> String {
    type_path(t).split("::").last().unwrap_or("").into()
}
fn attr_string(a: &Attribute) -> Option<String> {
    if let Meta::NameValue(n) = &a.meta {
        if let syn::Expr::Lit(l) = &n.value {
            if let syn::Lit::Str(s) = &l.lit {
                return Some(s.value());
            }
        }
    }
    None
}
fn is_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path()
            .segments
            .last()
            .is_some_and(|s| matches!(s.ident.to_string().as_str(), "test" | "rstest"))
    })
}
fn named_cases(attrs: &[Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter(|a| a.path().segments.first().is_some_and(|s| s.ident == "case"))
        .filter_map(|a| a.path().segments.iter().nth(1).map(|s| ident(&s.ident)))
        .collect()
}
fn test_class(name: &str) -> &str {
    ["positive", "negative", "corner", "boundary", "adversarial"]
        .into_iter()
        .find(|p| name.starts_with(&format!("{p}_")))
        .unwrap_or("other")
}
fn fn_attrs(sig: &syn::Signature) -> Value {
    json!({"async": sig.asyncness.is_some(), "unsafe": sig.unsafety.is_some(),
        "params": clipped(&sig.inputs.to_token_stream().to_string(), 256),
        "ret": clipped(&sig.output.to_token_stream().to_string(), 256)})
}
fn cfg_features(meta: &Meta, features: &mut BTreeSet<String>) {
    match meta {
        Meta::NameValue(n) if n.path.is_ident("feature") => {
            if let syn::Expr::Lit(l) = &n.value {
                if let syn::Lit::Str(s) = &l.lit {
                    features.insert(s.value());
                }
            }
        }
        Meta::List(l) if !l.path.is_ident("not") => {
            if let Ok(children) =
                Punctuated::<Meta, Token![,]>::parse_terminated.parse2(l.tokens.clone())
            {
                for child in children {
                    cfg_features(&child, features);
                }
            }
        }
        _ => {}
    }
}
fn attributes(item: &Item) -> &[Attribute] {
    match item {
        Item::Fn(v) => &v.attrs,
        Item::Struct(v) => &v.attrs,
        Item::Enum(v) => &v.attrs,
        Item::Trait(v) => &v.attrs,
        Item::Type(v) => &v.attrs,
        Item::Const(v) => &v.attrs,
        Item::Static(v) => &v.attrs,
        Item::Macro(v) => &v.attrs,
        Item::Impl(v) => &v.attrs,
        Item::Mod(v) => &v.attrs,
        Item::Use(v) => &v.attrs,
        _ => &[],
    }
}

fn without_outer_attrs(tokens: TokenStream) -> TokenStream {
    let mut tokens = tokens.into_iter().peekable();
    while matches!(tokens.peek(), Some(TokenTree::Punct(p)) if p.as_char() == '#') {
        tokens.next();
        if matches!(tokens.peek(), Some(TokenTree::Punct(p)) if p.as_char() == '!') {
            tokens.next();
        }
        tokens.next();
    }
    tokens.collect()
}
fn strip_docs(tokens: TokenStream) -> TokenStream {
    let mut result = TokenStream::new();
    let mut tokens = tokens.into_iter().peekable();
    while let Some(token) = tokens.next() {
        if matches!(&token, TokenTree::Punct(p) if p.as_char() == '#') {
            let mut candidate = tokens.clone();
            if matches!(candidate.peek(), Some(TokenTree::Punct(p)) if p.as_char() == '!') {
                candidate.next();
            }
            if let Some(TokenTree::Group(group)) = candidate.peek() {
                if group.delimiter() == proc_macro2::Delimiter::Bracket
                    && matches!(group.stream().into_iter().next(), Some(TokenTree::Ident(i)) if i == "doc")
                {
                    candidate.next();
                    tokens = candidate;
                    continue;
                }
            }
        }
        let token = if let TokenTree::Group(group) = token {
            TokenTree::Group(proc_macro2::Group::new(
                group.delimiter(),
                strip_docs(group.stream()),
            ))
        } else {
            token
        };
        result.extend([token]);
    }
    result
}
