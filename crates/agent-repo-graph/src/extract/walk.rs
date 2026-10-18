//! Disable the walker's implicit file reads: even `.gitignore` is repository input.
//! Read local ignore rules only after confinement and byte limits, never host/global rules.
use super::{clipped, Context};
use crate::ExtractBudget;
use ignore::{
    gitignore::{Gitignore, GitignoreBuilder},
    WalkBuilder,
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

struct Rules {
    root: PathBuf,
    cap: u64,
    max_dirs: usize,
    cache: BTreeMap<PathBuf, Gitignore>,
    skipped: usize,
}

impl Rules {
    fn ignored(&mut self, path: &Path, is_dir: bool) -> bool {
        let mut dirs = Vec::new();
        let mut parent = path.parent();
        while let Some(dir) = parent.filter(|p| p.starts_with(&self.root)) {
            dirs.push(dir.to_owned());
            parent = dir.parent();
            if dirs.len() > 128 {
                self.skipped += 1;
                return true;
            }
        }
        let mut ignored = false;
        for dir in dirs.into_iter().rev() {
            if !self.cache.contains_key(&dir) {
                if self.cache.len() >= self.max_dirs {
                    self.skipped += 1;
                    return true;
                }
                let mut builder = GitignoreBuilder::new(&dir);
                let path = dir.join(".gitignore");
                if path.symlink_metadata().is_ok() {
                    let safe = path
                        .strip_prefix(&self.root)
                        .ok()
                        .and_then(Path::to_str)
                        .and_then(|p| agent_core::confine(&self.root, p).ok());
                    let text = safe.and_then(|p| {
                        if p.symlink_metadata().ok()?.file_type().is_symlink() {
                            return None;
                        }
                        let file = File::open(&p).ok()?;
                        if !file.metadata().ok()?.is_file()
                            || file.metadata().ok()?.len() > self.cap
                        {
                            return None;
                        }
                        let mut bytes = Vec::new();
                        file.take(self.cap + 1).read_to_end(&mut bytes).ok()?;
                        if bytes.len() as u64 > self.cap {
                            return None;
                        }
                        String::from_utf8(bytes).ok()
                    });
                    if let Some(text) = text {
                        for line in text.lines() {
                            if builder.add_line(Some(path.clone()), line).is_err() {
                                self.skipped += 1;
                            }
                        }
                    } else {
                        self.skipped += 1;
                    }
                }
                let matcher = builder.build().unwrap_or_else(|_| {
                    self.skipped += 1;
                    Gitignore::empty()
                });
                self.cache.insert(dir.clone(), matcher);
            }
            if let Some(rules) = self.cache.get(&dir) {
                let result = rules.matched(path, is_dir);
                if !result.is_none() {
                    ignored = result.is_ignore();
                }
            }
        }
        ignored
    }
}

pub(super) fn inventory(
    cx: &mut Context<'_>,
    budget: &ExtractBudget,
) -> crate::RepoGraphResult<()> {
    let root = cx
        .root
        .canonicalize()
        .map_err(|_| crate::RepoGraphError::Invalid("extraction root".into()))?;
    let rules = Arc::new(Mutex::new(Rules {
        root: root.clone(),
        cap: budget.max_file_bytes.min(2 * 1024 * 1024) as u64,
        max_dirs: budget.max_files.saturating_add(1),
        cache: BTreeMap::new(),
        skipped: 0,
    }));
    let filtered = rules.clone();
    let walk = WalkBuilder::new(&root)
        .standard_filters(false)
        .follow_links(false)
        .sort_by_file_path(Path::cmp)
        .filter_entry(move |entry| {
            if entry.depth() == 0 {
                return true;
            }
            if ["target", ".git", ".agent-seddon", "node_modules", "vendor"]
                .iter()
                .any(|n| entry.file_name() == *n)
            {
                return false;
            }
            let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
            !filtered
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .ignored(entry.path(), is_dir)
        })
        .build();
    for entry in walk {
        cx.check()?;
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                cx.skip();
                continue;
            }
        };
        let Some(ft) = entry.file_type() else {
            cx.skip();
            continue;
        };
        if ft.is_symlink() {
            cx.skip();
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let Some(path) = entry.path().strip_prefix(&root).ok().and_then(Path::to_str) else {
            cx.skip();
            continue;
        };
        if cx.files.len() >= budget.max_files {
            cx.report.truncated = true;
            cx.report.skipped += 1;
            break;
        }
        if crate::NodeKey::file(path).is_err() || agent_core::confine(&root, path).is_err() {
            cx.skip();
            continue;
        }
        cx.files.insert(path.to_owned());
    }
    let skipped = rules
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .skipped;
    cx.report.skipped += skipped;
    cx.report.truncated |= skipped > 0;
    cx.report.files = cx.files.len();
    if skipped > 0 {
        cx.report.diagnostics.push(clipped(
            "Ignore rules unavailable, invalid, or beyond walk budget",
            200,
        ));
    }
    cx.check()
}
