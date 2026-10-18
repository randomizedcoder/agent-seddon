use super::{clipped, hash, Context};
use crate::{EdgeKind, GraphBuilder, NodeKey, NodeSpec, RepoGraphResult};
use serde_json::json;
use std::{collections::BTreeMap, path::Path};

pub(super) struct Package {
    pub name: String,
    pub manifest: String,
    pub value: toml::Value,
    pub roots: Vec<(String, String)>,
}

pub(super) struct Catalog {
    pub packages: Vec<Package>,
    workspace: toml::Value,
}

fn empty() -> toml::Value {
    toml::Value::Table(Default::default())
}

fn strings(value: Option<&toml::Value>) -> Vec<String> {
    value
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn matches(patterns: &[String], dir: &str) -> bool {
    patterns.iter().any(|p| {
        globset::Glob::new(p)
            .ok()
            .is_some_and(|g| g.compile_matcher().is_match(dir))
    })
}

pub(super) fn relative(cx: &Context<'_>, base: &str, path: &str) -> Option<String> {
    let joined = Path::new(base).join(path);
    let confined = agent_core::confine(cx.root, joined.to_str()?).ok()?;
    let real = confined.canonicalize().ok()?;
    let root = cx.root.canonicalize().ok()?;
    let rel = real.strip_prefix(root).ok()?.to_str()?.to_owned();
    // Canonicalization and graph key validation also reject non-UTF8 or hostile paths.
    Some(rel)
}

impl Catalog {
    pub fn read(cx: &mut Context<'_>) -> RepoGraphResult<Self> {
        let mut manifests = BTreeMap::new();
        for file in cx
            .files
            .clone()
            .iter()
            .filter(|f| Path::new(f).file_name().is_some_and(|n| n == "Cargo.toml"))
        {
            cx.check()?;
            let Some(text) = cx.text(file)? else {
                continue;
            };
            match text.parse::<toml::Value>() {
                Ok(value) => {
                    manifests.insert(file.clone(), value);
                }
                Err(_) => cx.parse_error(),
            }
        }
        let workspace = manifests.get("Cargo.toml").cloned().unwrap_or_else(empty);
        let members = strings(workspace.get("workspace").and_then(|v| v.get("members")));
        let excludes = strings(workspace.get("workspace").and_then(|v| v.get("exclude")));
        let has_workspace = workspace.get("workspace").is_some();
        let mut packages = Vec::new();
        for (manifest, value) in manifests {
            let dir = Path::new(&manifest)
                .parent()
                .and_then(Path::to_str)
                .unwrap_or("");
            if manifest != "Cargo.toml"
                && (!has_workspace || !matches(&members, dir) || matches(&excludes, dir))
            {
                continue;
            }
            let Some(pkg) = value.get("package") else {
                continue;
            };
            let Some(name) = pkg.get("name").and_then(toml::Value::as_str) else {
                cx.parse_error();
                continue;
            };
            let name = name.replace('-', "_");
            if NodeKey::rust_crate(&name).is_err() {
                cx.skip();
                continue;
            }
            let mut roots = Vec::new();
            let lib = value
                .get("lib")
                .and_then(|v| v.get("path"))
                .and_then(toml::Value::as_str)
                .unwrap_or("src/lib.rs");
            if let Some(path) = relative(cx, dir, lib).filter(|p| cx.files.contains(p)) {
                roots.push((path, String::new()));
            }
            let bins = value.get("bin").and_then(toml::Value::as_array);
            if let Some(bins) = bins {
                for bin in bins {
                    let bin_name = bin
                        .get("name")
                        .and_then(toml::Value::as_str)
                        .unwrap_or(&name)
                        .replace('-', "_");
                    if let Some(path) = bin
                        .get("path")
                        .and_then(toml::Value::as_str)
                        .and_then(|p| relative(cx, dir, p))
                        .filter(|p| cx.files.contains(p))
                    {
                        roots.push((path, format!("__bin_{bin_name}")));
                    }
                }
            }
            if let Some(path) = relative(cx, dir, "src/main.rs")
                .filter(|p| cx.files.contains(p) && !roots.iter().any(|(r, _)| r == p))
            {
                roots.push((path, format!("__bin_{name}")));
            }
            // Cargo's implicit top-level integration tests and binary targets are separate
            // Rust namespaces. Prefix their module keys to avoid colliding with library items.
            for file in &cx.files {
                for (subdir, prefix) in [("tests", "__test_"), ("src/bin", "__bin_")] {
                    let parent = Path::new(dir).join(subdir);
                    if Path::new(file).parent() == Some(parent.as_path())
                        && file.ends_with(".rs")
                        && !roots.iter().any(|(r, _)| r == file)
                    {
                        if let Some(stem) = Path::new(file).file_stem().and_then(|n| n.to_str()) {
                            roots.push((
                                file.clone(),
                                format!("{prefix}{}", stem.replace('-', "_")),
                            ));
                        }
                    }
                }
            }
            roots.sort();
            packages.push(Package {
                name,
                manifest,
                value,
                roots,
            });
        }
        Ok(Self {
            packages,
            workspace,
        })
    }

    pub fn emit(&self, cx: &mut Context<'_>, out: &mut GraphBuilder) -> RepoGraphResult<()> {
        let by_dir: BTreeMap<_, _> = self
            .packages
            .iter()
            .map(|p| {
                (
                    Path::new(&p.manifest)
                        .parent()
                        .and_then(Path::to_str)
                        .unwrap_or(""),
                    p,
                )
            })
            .collect();
        for pkg in &self.packages {
            cx.check()?;
            let key = NodeKey::rust_crate(&pkg.name)?;
            let manifest = &pkg.value;
            let package = &manifest["package"];
            let inherited = |field: &str| {
                let v = package.get(field);
                let v = if v
                    .and_then(|v| v.get("workspace"))
                    .and_then(toml::Value::as_bool)
                    == Some(true)
                {
                    self.workspace
                        .get("workspace")
                        .and_then(|v| v.get("package"))
                        .and_then(|v| v.get(field))
                } else {
                    v
                };
                clipped(v.and_then(toml::Value::as_str).unwrap_or(""), 256)
            };
            let mut external = 0;
            let dir = Path::new(&pkg.manifest)
                .parent()
                .and_then(Path::to_str)
                .unwrap_or("");
            let mut sections = vec![manifest];
            if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
                sections.extend(targets.values());
            }
            for section in sections {
                for group in ["dependencies", "dev-dependencies", "build-dependencies"] {
                    for (alias, dep) in section
                        .get(group)
                        .and_then(toml::Value::as_table)
                        .into_iter()
                        .flatten()
                    {
                        let inherited =
                            dep.get("workspace").and_then(toml::Value::as_bool) == Some(true);
                        let resolved = if inherited {
                            self.workspace
                                .get("workspace")
                                .and_then(|v| v.get("dependencies"))
                                .and_then(|v| v.get(alias))
                                .unwrap_or(dep)
                        } else {
                            dep
                        };
                        let target = resolved
                            .get("path")
                            .and_then(toml::Value::as_str)
                            .and_then(|path| relative(cx, if inherited { "" } else { dir }, path))
                            .and_then(|path| by_dir.get(path.as_str()).copied());
                        if let Some(target) = target {
                            out.edge(EdgeKind::DependsOn, &key, &NodeKey::rust_crate(&target.name)?, 1.0,
                                json!({"dev": group == "dev-dependencies", "build": group == "build-dependencies",
                                    "optional": dep.get("optional").and_then(toml::Value::as_bool).unwrap_or(false),
                                    "features": strings(dep.get("features")).into_iter().map(|s| clipped(&s, 128)).take(32).collect::<Vec<_>>() }));
                        } else {
                            external += 1;
                        }
                    }
                }
            }
            cx.node(out, NodeSpec::new(key.clone(), &pkg.name).with_file(&pkg.manifest)
                .with_body(hash(manifest.to_string())).with_attrs(json!({
                    "version": inherited("version"), "edition": inherited("edition"), "external_deps": external,
                    "lib": pkg.roots.iter().any(|(_, m)| m.is_empty()),
                    "bins": pkg.roots.iter().filter(|(_, m)| m.starts_with("__bin_")).map(|(p, _)| clipped(p, 256)).take(16).collect::<Vec<_>>()
                })));
            for (name, _) in manifest
                .get("features")
                .and_then(toml::Value::as_table)
                .into_iter()
                .flatten()
            {
                let Ok(feature) = NodeKey::rust_feature(&pkg.name, name) else {
                    cx.skip();
                    continue;
                };
                cx.node(
                    out,
                    NodeSpec::new(feature.clone(), name).with_file(&pkg.manifest),
                );
                out.edge(EdgeKind::Contains, &key, &feature, 1.0, json!({}));
            }
        }
        Ok(())
    }
}
