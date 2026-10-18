use super::{
    cargo::{relative, Catalog},
    hash, screened, Context,
};
use crate::{EdgeKind, GraphBuilder, NodeKey, NodeSpec, RepoGraphResult};
use serde_json::json;
use std::{collections::BTreeSet, path::Path};

pub(super) fn emit(
    cx: &mut Context<'_>,
    out: &mut GraphBuilder,
    catalog: &Catalog,
) -> RepoGraphResult<()> {
    for file in cx.files.clone().iter().filter(|p| {
        matches!(p.as_str(), "README.md" | "CLAUDE.md" | "DESIGN.md")
            || (p.starts_with("docs/") && p.ends_with(".md"))
    }) {
        let Some(text) = cx.text(file)? else {
            continue;
        };
        let key = NodeKey::doc(file)?;
        let mut targets = BTreeSet::new();
        // Only local links and code spans containing a path are evidence. Never open URLs.
        for rest in text.split("](").skip(1) {
            if let Some((link, _)) = rest.split_once(')') {
                targets.insert(
                    link.split_whitespace()
                        .next()
                        .unwrap_or("")
                        .trim_matches(['<', '>'])
                        .to_owned(),
                );
            }
        }
        for (i, span) in text.split('`').enumerate() {
            if i % 2 == 1 && span.contains('/') && !span.contains(char::is_whitespace) {
                targets.insert(span.to_owned());
            }
        }
        let mut broken = 0;
        let base = Path::new(file)
            .parent()
            .and_then(Path::to_str)
            .unwrap_or("");
        for target in targets {
            let target = target.split('#').next().unwrap_or("");
            if target.is_empty() || target.contains("://") || target.starts_with("mailto:") {
                continue;
            }
            let path = relative(cx, base, target).or_else(|| relative(cx, "", target));
            let Some(path) = path else {
                broken += 1;
                continue;
            };
            let crate_target = catalog
                .packages
                .iter()
                .find(|pkg| Path::new(&pkg.manifest).parent() == Some(Path::new(&path)));
            let target_key = if let Some(pkg) = crate_target {
                NodeKey::rust_crate(&pkg.name).ok()
            } else if cx.files.contains(&path) {
                NodeKey::file(&path).ok()
            } else {
                None
            };
            if let Some(target_key) = target_key {
                out.edge(EdgeKind::Documents, &key, &target_key, 1.0, json!({}));
            } else {
                broken += 1;
            }
        }
        let title = text
            .lines()
            .find_map(|l| l.strip_prefix("# "))
            .unwrap_or("");
        cx.node(out, NodeSpec::new(key, super::clipped(file, 256)).with_file(file).with_body(hash(text.as_bytes()))
            .with_attrs(json!({"title": screened(title, 256), "headings": text.lines().filter(|l| l.starts_with('#')).count(), "broken_links": broken})));
    }
    Ok(())
}
