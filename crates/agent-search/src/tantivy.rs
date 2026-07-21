//! `search-tantivy` — full-text code search over a tantivy on-disk index.
//!
//! One [`TantivyBackend`] owns a single `Index`. Queries acquire a cheap,
//! cloneable `Searcher` snapshot and run off the read path (no lock), so many
//! queries run concurrently — including while a reindex is in flight (the
//! searcher serves the last committed segments until `reload` swaps in the new
//! ones). Indexing goes through the single `IndexWriter`, guarded by an async
//! mutex used only by [`TantivyBackend::reindex`] — never by queries.
//!
//! Freshness is tracked by a [`Manifest`] saved next to the index; a reindex is
//! incremental (delete-by-path + re-add the changed files) when a prior manifest
//! exists, and a full rebuild otherwise.

use crate::{manifest, Manifest};
use agent_core::{
    Error, IndexState, IndexStatus, ProgressFn, Result, SearchBackend, SearchCapabilities,
    SearchHit, SearchMode, SearchQuery,
};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tantivy::collector::{DocSetCollector, TopDocs};
use tantivy::directory::MmapDirectory;
use tantivy::query::{
    AllQuery, BooleanQuery, FuzzyTermQuery, Occur, PhraseQuery, Query, RegexQuery, TermQuery,
};
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TantivyDocument, Value, STORED, STRING, TEXT,
};
use tantivy::{doc, Index, IndexReader, IndexWriter, ReloadPolicy, Term};
use tokio::sync::Mutex;

/// Indexer heap budget. Comfortably above tantivy's per-thread minimum.
const WRITER_HEAP_BYTES: usize = 100_000_000;
/// Cap a hit's snippet so results stay compact in the model's context.
const SNIPPET_MAX: usize = 200;

/// The three indexed fields (all `Copy`).
#[derive(Clone, Copy)]
struct Fields {
    path: Field,
    content: Field,
    lang: Field,
}

/// A tantivy-backed [`SearchBackend`] for a repo rooted at `root`, with its index
/// under `index_dir` (typically `<root>/.agent-seddon/index/tantivy`).
pub struct TantivyBackend {
    root: PathBuf,
    index_dir: PathBuf,
    // The `IndexReader`/`IndexWriter` own everything the backend needs, so the
    // `Index` itself is not retained.
    reader: IndexReader,
    writer: Mutex<IndexWriter>,
    fields: Fields,
    building: AtomicBool,
}

impl TantivyBackend {
    /// Open (or create) the index for `root` at `index_dir`.
    pub fn open(root: PathBuf, index_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&index_dir)?;
        let mut sb = Schema::builder();
        sb.add_text_field("path", STRING | STORED);
        sb.add_text_field("content", TEXT | STORED);
        sb.add_text_field("lang", STRING | STORED);
        let schema = sb.build();

        let dir = MmapDirectory::open(&index_dir).map_err(se)?;
        let index = Index::open_or_create(dir, schema).map_err(se)?;
        let schema = index.schema();
        let fields = Fields {
            path: schema.get_field("path").map_err(se)?,
            content: schema.get_field("content").map_err(se)?,
            lang: schema.get_field("lang").map_err(se)?,
        };
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(se)?;
        let writer = index.writer(WRITER_HEAP_BYTES).map_err(se)?;

        Ok(Self {
            root,
            index_dir,
            reader,
            writer: Mutex::new(writer),
            fields,
            building: AtomicBool::new(false),
        })
    }

    fn manifest_path(&self) -> PathBuf {
        self.index_dir.join("manifest.json")
    }
}

#[async_trait]
impl SearchBackend for TantivyBackend {
    fn capabilities(&self) -> SearchCapabilities {
        SearchCapabilities {
            backend: "tantivy".into(),
            modes: vec![
                SearchMode::Literal,
                SearchMode::Phrase,
                SearchMode::Fuzzy,
                SearchMode::Regex,
            ],
            content_search: true,
            scored: true,
            incremental: true,
            max_concurrent_queries: 0,
        }
    }

    async fn status(&self) -> Result<IndexStatus> {
        let root = self.root.clone();
        let mpath = self.manifest_path();
        let building = self.building.load(Ordering::Relaxed);
        let status = tokio::task::spawn_blocking(move || {
            let stored = Manifest::load(&mpath);
            let state = if building {
                IndexState::Building
            } else {
                manifest::compare(&root, stored.as_ref())
            };
            let (files, last, digest) = match &stored {
                Some(m) => (m.entries.len() as u64, m.built_ms, m.digest()),
                None => (0, 0, String::new()),
            };
            IndexStatus {
                state,
                indexed_files: files,
                last_indexed_ms: last,
                manifest_digest: digest,
            }
        })
        .await
        .map_err(|e| Error::Search(format!("status task panicked: {e}")))?;
        Ok(status)
    }

    async fn reindex(&self, progress: ProgressFn<'_>) -> Result<IndexStatus> {
        self.building.store(true, Ordering::Relaxed);
        let result = self.reindex_inner(progress).await;
        self.building.store(false, Ordering::Relaxed);
        result
    }

    async fn query(&self, q: &SearchQuery) -> Result<Vec<SearchHit>> {
        let (query, matcher) = build_query(&self.fields, q)?;
        let globs = compile_globs(&q.path_globs)?;
        let searcher = self.reader.searcher();
        let fields = self.fields;
        let limit = q.limit.max(1);
        tokio::task::spawn_blocking(move || {
            run_search(searcher, query, limit, fields, matcher, globs)
        })
        .await
        .map_err(|e| Error::Search(format!("query task panicked: {e}")))?
    }

    async fn list_files(&self, globs: &[String]) -> Result<Vec<PathBuf>> {
        let globs = compile_globs(globs)?;
        let searcher = self.reader.searcher();
        let path_field = self.fields.path;
        tokio::task::spawn_blocking(move || {
            let addrs = searcher.search(&AllQuery, &DocSetCollector).map_err(se)?;
            let mut paths = Vec::with_capacity(addrs.len());
            for addr in addrs {
                let doc: TantivyDocument = searcher.doc(addr).map_err(se)?;
                if let Some(p) = doc.get_first(path_field).and_then(|v| v.as_str()) {
                    if globs.is_empty() || globs.iter().any(|g| g.is_match(p)) {
                        paths.push(PathBuf::from(p));
                    }
                }
            }
            paths.sort();
            paths.dedup();
            Ok(paths)
        })
        .await
        .map_err(|e| Error::Search(format!("list_files task panicked: {e}")))?
    }
}

impl TantivyBackend {
    async fn reindex_inner(&self, progress: ProgressFn<'_>) -> Result<IndexStatus> {
        // Walk the tree off the async executor — it can be large.
        let root = self.root.clone();
        let current = tokio::task::spawn_blocking(move || Manifest::scan(&root))
            .await
            .map_err(|e| Error::Search(format!("scan task panicked: {e}")))?;

        let stored = Manifest::load(&self.manifest_path());
        let (upserts, deletes) = diff(stored.as_ref(), &current);

        let mut writer = self.writer.lock().await;
        if stored.is_none() {
            // No prior manifest: rebuild from scratch so a partial/foreign index
            // can't leave stale docs behind.
            writer.delete_all_documents().map_err(se)?;
        }

        let total = upserts.len() as u64;
        for (i, rel) in upserts.iter().enumerate() {
            let rel_str = rel.to_string_lossy().to_string();
            // Delete-then-add makes the upsert idempotent (incremental update).
            writer.delete_term(Term::from_field_text(self.fields.path, &rel_str));
            if let Ok(content) = std::fs::read_to_string(self.root.join(rel)) {
                writer
                    .add_document(doc!(
                        self.fields.path => rel_str,
                        self.fields.content => content,
                        self.fields.lang => lang_of(rel),
                    ))
                    .map_err(se)?;
            }
            progress(agent_core::ReindexProgress {
                files_done: (i + 1) as u64,
                files_total: total,
                done: false,
            });
        }
        for rel in &deletes {
            writer.delete_term(Term::from_field_text(
                self.fields.path,
                &rel.to_string_lossy(),
            ));
        }
        writer.commit().map_err(se)?;
        drop(writer);
        self.reader.reload().map_err(se)?;

        current.save(&self.manifest_path())?;
        progress(agent_core::ReindexProgress {
            files_done: total,
            files_total: total,
            done: true,
        });

        Ok(IndexStatus {
            state: IndexState::Fresh,
            indexed_files: current.entries.len() as u64,
            last_indexed_ms: current.built_ms,
            manifest_digest: current.digest(),
        })
    }
}

/// Paths to (re)index and paths to delete, given the prior manifest and the
/// freshly-scanned one. With no prior manifest, everything is an upsert.
fn diff(stored: Option<&Manifest>, current: &Manifest) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let Some(stored) = stored else {
        return (current.entries.keys().cloned().collect(), Vec::new());
    };
    let upserts = current
        .entries
        .iter()
        .filter(|(p, st)| stored.entries.get(*p) != Some(*st))
        .map(|(p, _)| p.clone())
        .collect();
    let deletes = stored
        .entries
        .keys()
        .filter(|p| !current.entries.contains_key(*p))
        .cloned()
        .collect();
    (upserts, deletes)
}

/// How a result line is located for snippet/line reporting (distinct from the
/// tantivy term query, which matches at token granularity in the index).
enum LineMatcher {
    Contains(String),
    Regex(regex::Regex),
    /// No reliable line predicate (e.g. fuzzy) — report the file, line 0.
    None,
}

impl LineMatcher {
    fn matches(&self, line: &str) -> bool {
        match self {
            LineMatcher::Contains(needle) => line.to_lowercase().contains(needle),
            LineMatcher::Regex(re) => re.is_match(line),
            LineMatcher::None => false,
        }
    }
}

/// Build the tantivy query for a mode plus the line matcher used to locate the
/// snippet within a matched document.
fn build_query(fields: &Fields, q: &SearchQuery) -> Result<(Box<dyn Query>, LineMatcher)> {
    let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
    let matcher = match q.mode {
        SearchMode::Literal => {
            let toks = tokenize(&q.text);
            if toks.is_empty() {
                return Err(Error::Search("empty query".into()));
            }
            for t in &toks {
                clauses.push((Occur::Must, term_query(fields.content, t)));
            }
            LineMatcher::Contains(toks[0].clone())
        }
        SearchMode::Phrase => {
            let toks = tokenize(&q.text);
            if toks.is_empty() {
                return Err(Error::Search("empty query".into()));
            }
            if toks.len() == 1 {
                clauses.push((Occur::Must, term_query(fields.content, &toks[0])));
            } else {
                let terms = toks
                    .iter()
                    .map(|t| Term::from_field_text(fields.content, t))
                    .collect();
                clauses.push((Occur::Must, Box::new(PhraseQuery::new(terms))));
            }
            LineMatcher::Contains(toks[0].clone())
        }
        SearchMode::Fuzzy => {
            let toks = tokenize(&q.text);
            let first = toks
                .first()
                .ok_or_else(|| Error::Search("empty query".into()))?;
            let distance = q.fuzzy_distance.unwrap_or(1).min(2);
            let term = Term::from_field_text(fields.content, first);
            clauses.push((
                Occur::Must,
                Box::new(FuzzyTermQuery::new(term, distance, true)),
            ));
            LineMatcher::None
        }
        SearchMode::Regex => {
            let rq = RegexQuery::from_pattern(&q.text, fields.content)
                .map_err(|e| Error::Search(format!("invalid regex: {e}")))?;
            clauses.push((Occur::Must, Box::new(rq)));
            match regex::Regex::new(&q.text) {
                Ok(re) => LineMatcher::Regex(re),
                Err(_) => LineMatcher::None,
            }
        }
        // tantivy is lexical; semantic/hybrid are the vector backend's / dispatch's
        // job and are rejected up front by `reject_unsupported` (tantivy's caps
        // don't advertise them), so this arm is unreachable in practice.
        SearchMode::Semantic | SearchMode::Hybrid => {
            return Err(Error::Search(format!(
                "tantivy does not support {} search",
                q.mode.as_str()
            )));
        }
    };
    if let Some(lang) = &q.lang {
        let term = Term::from_field_text(fields.lang, &lang.to_lowercase());
        clauses.push((
            Occur::Must,
            Box::new(TermQuery::new(term, IndexRecordOption::Basic)),
        ));
    }
    Ok((Box::new(BooleanQuery::new(clauses)), matcher))
}

fn term_query(field: Field, text: &str) -> Box<dyn Query> {
    Box::new(TermQuery::new(
        Term::from_field_text(field, text),
        IndexRecordOption::WithFreqs,
    ))
}

fn run_search(
    searcher: tantivy::Searcher,
    query: Box<dyn Query>,
    limit: usize,
    fields: Fields,
    matcher: LineMatcher,
    globs: Vec<regex::Regex>,
) -> Result<Vec<SearchHit>> {
    let top = searcher
        .search(&query, &TopDocs::with_limit(limit).order_by_score())
        .map_err(se)?;
    let mut hits = Vec::with_capacity(top.len());
    for (score, addr) in top {
        let doc: TantivyDocument = searcher.doc(addr).map_err(se)?;
        let path = doc
            .get_first(fields.path)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if !globs.is_empty() && !globs.iter().any(|g| g.is_match(&path)) {
            continue;
        }
        let content = doc
            .get_first(fields.content)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let (line, snippet) = locate(content, &matcher);
        hits.push(SearchHit {
            path: PathBuf::from(path),
            line,
            col_start: 0,
            col_end: 0,
            score,
            snippet,
        });
    }
    Ok(hits)
}

/// First line satisfying the matcher (1-based), else `(0, first-line snippet)`.
fn locate(content: &str, matcher: &LineMatcher) -> (u32, String) {
    if !matches!(matcher, LineMatcher::None) {
        for (i, line) in content.lines().enumerate() {
            if matcher.matches(line) {
                return ((i + 1) as u32, snippet_of(line));
            }
        }
    }
    (0, snippet_of(content.lines().next().unwrap_or("")))
}

fn snippet_of(line: &str) -> String {
    let t = line.trim();
    if t.len() <= SNIPPET_MAX {
        return t.to_string();
    }
    let mut cut = SNIPPET_MAX;
    while cut > 0 && !t.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &t[..cut])
}

/// Split into lowercased alphanumeric tokens, matching the default tokenizer so
/// literal/phrase terms line up with what was indexed.
fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}

/// Map a file extension to a coarse language label (stored for the `lang` filter).
fn lang_of(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "rs" => "rust",
        "nix" => "nix",
        "py" => "python",
        "js" | "jsx" => "javascript",
        "ts" | "tsx" => "typescript",
        "go" => "go",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" => "cpp",
        "java" => "java",
        "rb" => "ruby",
        "sh" | "bash" => "shell",
        "toml" => "toml",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "md" | "markdown" => "markdown",
        "txt" => "text",
        other => other,
    }
    .to_string()
}

fn compile_globs(globs: &[String]) -> Result<Vec<regex::Regex>> {
    globs.iter().map(|g| glob_to_regex(g)).collect()
}

/// Translate a shell-ish glob into an anchored regex over the relative path.
/// Supports `**` (any depth), `*` (within a path segment), `?`, literal `/`.
fn glob_to_regex(glob: &str) -> Result<regex::Regex> {
    let mut re = String::from("^");
    let chars: Vec<char> = glob.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '*' && chars.get(i + 1) == Some(&'*') {
            // `**/` matches any number of leading path segments *including none*,
            // so `**/*.rs` also matches a root-level `main.rs`.
            if chars.get(i + 2) == Some(&'/') {
                re.push_str("(?:.*/)?");
                i += 3;
            } else {
                re.push_str(".*");
                i += 2;
            }
            continue;
        }
        match chars[i] {
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            '/' => re.push('/'),
            c if c.is_alphanumeric() => re.push(c),
            c => {
                // Escape any regex metacharacter.
                re.push('\\');
                re.push(c);
            }
        }
        i += 1;
    }
    re.push('$');
    regex::Regex::new(&re).map_err(|e| Error::Search(format!("invalid path glob `{glob}`: {e}")))
}

/// tantivy error → our `Error::Search`.
fn se<E: std::fmt::Display>(e: E) -> Error {
    Error::Search(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::tempdir;
    use futures_util::future::join_all;
    use rstest::rstest;
    use std::sync::Arc;

    /// A fixture repo with planted tokens across a few files + languages.
    fn fixture() -> PathBuf {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/main.rs"),
            "fn main() {\n    let FINDME_LITERAL = 1;\n    println!(\"the quick brown fox\");\n}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "// searchable helpers\npub fn helper() -> usize { 42 }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("notes.md"),
            "# Notes\nThe quick brown fox jumps.\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("flake.nix"),
            "{ description = \"FINDME_LITERAL\"; }\n",
        )
        .unwrap();
        dir
    }

    async fn indexed() -> (PathBuf, TantivyBackend) {
        let dir = fixture();
        let backend =
            TantivyBackend::open(dir.clone(), dir.join(".agent-seddon/index/tantivy")).unwrap();
        backend.reindex(&|_p| {}).await.unwrap();
        (dir, backend)
    }

    fn query(text: &str, mode: SearchMode) -> SearchQuery {
        SearchQuery {
            text: text.into(),
            mode,
            path_globs: vec![],
            lang: None,
            limit: 20,
            fuzzy_distance: None,
        }
    }

    #[tokio::test]
    async fn status_transitions_missing_fresh_stale() {
        let dir = fixture();
        let backend =
            TantivyBackend::open(dir.clone(), dir.join(".agent-seddon/index/tantivy")).unwrap();
        assert_eq!(backend.status().await.unwrap().state, IndexState::Missing);
        backend.reindex(&|_p| {}).await.unwrap();
        assert_eq!(backend.status().await.unwrap().state, IndexState::Fresh);
        std::fs::write(dir.join("new.rs"), "fn added() {}").unwrap();
        assert_eq!(backend.status().await.unwrap().state, IndexState::Stale);
        backend.reindex(&|_p| {}).await.unwrap();
        assert_eq!(backend.status().await.unwrap().state, IndexState::Fresh);
    }

    // (text, mode, expected substring in some hit's path) — None ⇒ expect no hits.
    #[rstest]
    #[case::literal_token("FINDME_LITERAL", SearchMode::Literal, Some("main.rs"))]
    #[case::literal_other_file("description", SearchMode::Literal, Some("flake.nix"))]
    #[case::phrase("quick brown fox", SearchMode::Phrase, Some("main.rs"))]
    #[case::fuzzy_near("serchable", SearchMode::Fuzzy, Some("lib.rs"))]
    #[case::regex_token("help.*", SearchMode::Regex, Some("lib.rs"))]
    #[case::no_match("zzzznotpresent", SearchMode::Literal, None)]
    #[tokio::test]
    async fn query_modes(
        #[case] text: &str,
        #[case] mode: SearchMode,
        #[case] expect_path: Option<&str>,
    ) {
        let (_dir, backend) = indexed().await;
        let hits = backend.query(&query(text, mode)).await.unwrap();
        match expect_path {
            Some(p) => assert!(
                hits.iter().any(|h| h.path.to_string_lossy().contains(p)),
                "expected a hit in `{p}` for `{text}`, got {:?}",
                hits.iter().map(|h| h.path.clone()).collect::<Vec<_>>()
            ),
            None => assert!(hits.is_empty(), "expected no hits, got {}", hits.len()),
        }
    }

    #[tokio::test]
    async fn limit_is_respected() {
        let (_dir, backend) = indexed().await;
        let mut q = query("fox", SearchMode::Literal);
        q.limit = 1;
        assert!(backend.query(&q).await.unwrap().len() <= 1);
    }

    #[tokio::test]
    async fn path_glob_filters_results() {
        let (_dir, backend) = indexed().await;
        let mut q = query("FINDME_LITERAL", SearchMode::Literal);
        q.path_globs = vec!["**/*.nix".into()];
        let hits = backend.query(&q).await.unwrap();
        assert!(
            hits.iter()
                .all(|h| h.path.to_string_lossy().ends_with(".nix")),
            "glob must restrict to .nix, got {:?}",
            hits.iter().map(|h| h.path.clone()).collect::<Vec<_>>()
        );
        assert!(!hits.is_empty(), "flake.nix contains the token");
    }

    #[tokio::test]
    async fn lang_filter_restricts_results() {
        let (_dir, backend) = indexed().await;
        let mut q = query("FINDME_LITERAL", SearchMode::Literal);
        q.lang = Some("rust".into());
        let hits = backend.query(&q).await.unwrap();
        assert!(hits
            .iter()
            .all(|h| h.path.to_string_lossy().ends_with(".rs")));
    }

    // The load-bearing test: many concurrent queries while a reindex runs must
    // all succeed and never deadlock (serve-stale, lock-free read path).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_queries_during_reindex() {
        let (_dir, backend) = indexed().await;
        let backend = Arc::new(backend);
        let reindexer = {
            let b = backend.clone();
            tokio::spawn(async move { b.reindex(&|_p| {}).await })
        };
        let queries = (0..64).map(|_| {
            let b = backend.clone();
            async move { b.query(&query("fox", SearchMode::Literal)).await }
        });
        let results = join_all(queries).await;
        assert!(
            results.iter().all(|r| r.is_ok()),
            "all queries must succeed"
        );
        assert!(reindexer.await.unwrap().is_ok());
    }

    // --- pure helpers ------------------------------------------------------
    #[rstest]
    #[case::rs("a/b.rs", "rust")]
    #[case::nix("flake.nix", "nix")]
    #[case::md("README.md", "markdown")]
    #[case::unknown("x.zzz", "zzz")]
    #[case::none("Makefile", "")]
    fn lang_of_cases(#[case] path: &str, #[case] expected: &str) {
        assert_eq!(lang_of(Path::new(path)), expected);
    }

    #[rstest]
    #[case::ext("**/*.rs", "src/main.rs", true)]
    #[case::ext_root("**/*.nix", "flake.nix", true)]
    #[case::ext_reject("**/*.rs", "src/main.py", false)]
    #[case::single_segment("*.nix", "flake.nix", true)]
    #[case::single_segment_no_cross("*.nix", "a/flake.nix", false)]
    fn glob_cases(#[case] glob: &str, #[case] path: &str, #[case] expected: bool) {
        assert_eq!(glob_to_regex(glob).unwrap().is_match(path), expected);
    }
}
