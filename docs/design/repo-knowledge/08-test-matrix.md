# 08 — Test matrix

The table-driven cases behind the repo-knowledge crates, in the four case classes
(`positive_` / `negative_` / `corner_` / `boundary_`) plus the mandatory `adversarial_` class
for every untrusted input (repo content, model-supplied node keys, tenant strings). Each row
below maps to one named `rstest` case; the harness legend at the foot explains the store tier
stamps.

Owned by RK-01: **R1** (key grammar), **R2** (the builder), **R3** (store conformance). Owned by
RK-02: **P1** (the `PgRepoGraph` in-gate units — the pure helpers and the tenant handle, no DB) and
**R4** (the `PgRepoGraph` pg-only rows). Later increments add their own tables here (the extractor
tables in RK-03, the tool R5 in RK-08, …).

## R1 — key grammar, ids, tokens

Pure, `agent_core::repo_graph::key`.

| case | input | expected |
|---|---|---|
| `positive_parse_form` (one `#[case]` per grammar row: repo, file, doc, rust_crate, rust_feature, rust_mod, rust_fn, rust_impl_trait, rust_impl_inherent (`#-`), rust_method, rust_test_case, go_package, go_func, go_method, go_test, proto_rpc, sql_table, cfg, metric, span) | the doc's example key | parses; `kind()` / `lang()` as the grammar table says |
| `positive_constructor_round_trip` (one `#[case]` per typed constructor) | typed parts | `parse(built.as_str())` equals it, same kind |
| `positive_dup_suffix` | `rust:fn:a::b::f@0123abcd` | accepted, kind `fn` |
| `negative_reject` (unknown prefix, unknown rust kind, empty, prefix-only `rust:fn:`, `rust:`, a bare word) | Invalid grammar variants listed in the case name | `Invalid`; the message names the rule, not the input |
| `corner_go_interface_is_trait` / `corner_go_func_is_fn` | `go:interface:p.I`, `go:func:p.F` | kinds `trait` / `fn` |
| `boundary_key_512` / `boundary_key_513` | Keys exactly 512 and 513 bytes long | ok / `TooLong("node_key")` |
| `adversarial_charset` (whitespace, tab, control char, non-ASCII, bidi override, bad dup suffix `@zz`, double `@`, uppercase dup) | One key per prohibited character/suffix variant | rejected; the message echoes none of the input |
| `adversarial_file_path` (`file:../x`, absolute, backslash, `./`, `//`, `doc:a/../b`) and `repo_relative_rows` | One escaping or non-canonical path per listed variant | rejected |
| `adversarial_constructor_segment_colon` / `_segment_whitespace` / `_crate_dash` (`agent-core` must be `agent_core`) / `_rust_item_bad_kind` | One unsafe constructor component per listed variant | constructor returns `Invalid` |
| `positive_id_known_vector` (`file:a`), `positive_id_deterministic`, `corner_id_high_bit_negative` | Known file key, repeated hashing, and a key with the digest high bit set | the sha256[..8] as i64; equal across calls; a top-bit key gives a negative id |
| `positive_tokens` (snake, camel, acronym `HTTPServer`→`http,server`, mixed `PgDigests`, dotted), `corner_tokens_dedup`, `boundary_tokens_16` (17 parts ⇒ 16), `adversarial_tokens_huge_name` (10 KiB ⇒ empty), `adversarial_tokens_unicode` (non-ASCII dropped) | Listed casing/separator variants, duplicates, 17 parts, huge and Unicode names | Expected lowercase ASCII tokens; deduplicated; at most 16 tokens; huge name empty; Unicode parts dropped |

## R2 — `GraphBuilder` + validation

Pure, `agent_core::repo_graph::builder`.

| case | input | expected |
|---|---|---|
| `positive_sorted_output` | nodes / edges inserted out of order | `nodes()` by key, `edges()` by `(kind, src, dst)` |
| `positive_hash_deterministic` | the same items in two orders | equal `graph_hash` |
| `positive_hash_tracks_body` / `_sig`, `corner_hash_ignores_attrs_and_lines` | one field changed | hash changes / hash unchanged |
| `positive_cfg_dup_suffixed` | same key, cfg `["feature=a"]` then `["feature=b"]` | second `Suffixed(key@sha8)`, `attrs.dup = true`, `attrs.cfg` recorded |
| `negative_dup_same_cfg_fails` | same key, same cfg | `finish` ⇒ `BuildError::DuplicateKey` |
| `negative_id_collision_fails` (`with_id_fn(\|_\| NodeId(7))`) | two keys, one id | `BuildError::IdCollision` |
| `negative_dangling_edge_dropped` | edge to a key never added | edge absent, `dropped_edges = 1` |
| `corner_edge_before_node_kept` | edge, then both nodes | edge present |
| `corner_empty_graph` | nothing | `finish` ok, the constant empty hash, zero counts |
| `boundary_max_nodes` / `_max_edges` (caps 3 / 3) | 4 inserts | 3 kept, `truncated`, one dropped and counted |
| `boundary_name_256` / `_257`, `_qualifier_512` / `_513`, `_attrs_4096` / `_4097`, `_edge_attrs_1024` / `_1025` | Each named field at its maximum and one byte over | kept / `Dropped(DropReason::…)` naming the field |
| `adversarial_file_traversal` / `_file_absolute` / `_file_control` / `_attrs_not_object` / `_weight_nan` / `_weight_negative` / `_lines_reversed` | Traversing/absolute/control paths, non-object attrs, hostile weights and reversed line range | node dropped / dropped / dropped / dropped / weight `1.0` / `1.0` / `line_end = line_start` |
| `adversarial_drop_reason_never_echoes` | a 4 KiB hostile name | `DropReason` `Display` contains no byte of it |

## R3 — store conformance

`MemRepoGraph` now; `PgRepoGraph` reruns every row through `repo_graph_conformance_suite!` in
RK-02. `agent_testkit::repo_graph::conformance`.

| case | scenario | expected |
|---|---|---|
| `positive_repo_put_get` / `_upsert` / `positive_repos_sorted` / `negative_repo_get_unknown` | Create/read/upsert one repo; list several repos; read absent slug | ids stable across upsert, fields updated, `None` |
| `adversarial_repo_slug_unsafe` (`../x`, `a b`, 129 chars, empty), `_repo_profile_huge`, `_repo_remote_control_char` | Unsafe slug, profile above size limit, remote with control character | `Invalid` / `TooLong` naming the field |
| `adversarial_repo_cross_tenant` | put under `ta`, get under `tb` | `None`; `repos()` under `tb` empty |
| `positive_snapshot_lifecycle` | begin → write v1 → finish Ready | `Snapshot` has hash, counts, `ready`, `duration_ms` |
| `positive_snapshot_find_by_sha` / `_latest_skips_failed_and_building` / `_snapshots_newest_first_limited` | Ready, building and failed snapshots across different SHAs; bounded newest-first listing | Exact ready SHA selected; latest ignores non-ready; list ordered newest first and respects limit |
| `positive_bodies_shared` | two snapshots of v1 | same `NodeId`s, `nodes_by_key` identical on both |
| `positive_snapshot_diff` | v1 vs v2 | `added=[delta]`, `removed=[S,…]`, `sig_changed=[gamma]`, `body_changed=[beta]`, edges as built |
| `positive_retention` | 4 ready + 1 failed, keep 2 | returns 3; the two newest readable, the rest `NotFound` |
| `negative_begin_unknown_repo` / `_duplicate_identity` / `corner_begin_replaces_failed` / `negative_begin_bad_sha` / `adversarial_begin_extractor_name` | Unknown repo, repeated identity, failed prior attempt, malformed SHA, hostile extractor name | `NotFound` / `Conflict` / new id / `Invalid` |
| `negative_write_after_finish` / `_finish_twice` / `_finish_unknown` / `corner_write_empty_graph` / `negative_write_id_collision` | Finished/unknown snapshot, empty graph, and forced collision variants | `Conflict` / `Conflict` / `NotFound` / ok, zero counts / `Conflict`, snapshot still `building`, nothing written |
| `adversarial_snapshot_cross_repo` / `_cross_tenant` / `_diff_cross_repo` | mismatched scope pairs | `NotFound` |
| `positive_nodes_by_key` / `_by_file` / `_by_name_kind_filter` / `negative_unknown_key_empty` / `adversarial_name_huge` / `_name_whitespace` | Fixture keys/files/names and kind filter, absent key, oversized/whitespace query | rows / empty, no error |
| `positive_neighbors_out_calls_2_hops` / `_neighbors_in` / `corner_neighbors_cycle_terminates` / `boundary_hops_clamped` / `_cap_clamped` / `_keys_64` | Fixture outgoing/incoming paths, cycle, out-of-range hop/cap values and key limit | depths and clamps as specified |
| `positive_blast_radius` / `_tests_covering_via` / `_path_between` / `negative_path_none` / `corner_path_cycle_guard` / `boundary_paths_clamped` | Fixture dependents, name/SCIP test evidence, connected/disconnected paths, cycle and path limit | Expected dependent/test/path sets with evidence provenance; no path is empty; cycles terminate; path count capped |
| `positive_shape` | v1 | counts by kind and edge kind, `files = 2`, `crates = 1` |

Mem-only rows (`agent-testkit/src/repo_graph/tests.rs`): `adversarial_tenant_unsafe`,
`positive_tenant_handles_share_state`, `positive_clock_stamps_rows`, and the suite stamp
`repo_graph_conformance_suite!(mem, Harness::mem())`.

## P1 — `PgRepoGraph` in-gate units (no DB)

Behind `repo-graph-postgres`; run by `nix/checks/repo-graph.nix` (`cargo test --features
repo-graph-postgres`, **not** `--ignored`), so the pure helpers and the tenant handle are exercised
in `nix flake check` with no server. The pure-helper rows live in `crates/agent-repo-graph/src/postgres/sql.rs`;
the handle / `map_db` rows in `crates/agent-repo-graph/src/postgres/tests.rs`.

| case | scenario | expected |
|---|---|---|
| `positive_with_tenant_valid` / `_clone_shares_pool` | `with_tenant` on a lazily-connecting pool (no connection opened) | `Ok`, `tenant()` set; clone is a cheap `Arc` share |
| `adversarial_tenant_refused_before_any_statement` (`../x`, `a b`, `a/b`, `-x`, `..`, empty) | unsafe tenant on the lazy pool | `Invalid`, **before any statement** (provably synchronous) |
| `boundary_tenant_128_ok_129_refused` | 128- vs 129-char tenant | `Ok` / `Invalid` |
| `clamp_hops_rows` (0/99/mid × neighbor/radius/path max) | `clamp_hops` | `1` / the tier max / passthrough |
| `clamp_cap_rows` / `clamp_keep_rows` (0 / over / mid) | `clamp_cap` (`MAX_RESULT`), `clamp_keep` (`MAX_RETAIN`) | `1` / cap / passthrough |
| `boundary_ms_u64_max` / `boundary_unsigned_i64_negative` / `boundary_i32_of_saturates` | the `ms` / `unsigned` / `i32_of` binds | clamped, no panic/overflow |
| `positive_node_arrays_aligned` / `positive_edge_arrays_aligned` | `node_arrays` / `edge_arrays` on `fixture_v1` | every column `Vec` equal length; `ids[i] == node_id_for(keys[i])`; kinds are the enum `as_str`; default weight `1.0` |
| `positive_attrs_serialize_roundtrip` | node/edge `attrs` → JSON text column | each element parses back to the same `serde_json` object |
| `corner_empty_graph_arrays` | the array builders on an empty graph | all `Vec`s empty, no panic |
| `chunk_ranges` rows (`boundary_write_chunking` 25k/10k, exact-multiple, under-one-chunk, zero-len, `corner_chunk_zero_size_floored_to_one`) | `chunk_ranges(len, chunk)` | N batches covering every row exactly once; a zero chunk floors to one |
| `positive_map_db_row_not_found` / `negative_map_db_other_is_backend` | `map_db` on `RowNotFound` / a non-`Database` error | `NotFound` / `Backend(_)` carrying **no** DSN/password |

## R4 — `PgRepoGraph` pg-only rows (live; `#[ignore]`)

Not expressible against `MemRepoGraph`; `crates/agent-repo-graph/src/postgres/tests.rs`, gated on
`AGENT_REPO_GRAPH_TEST_DSN`, run via `nix run .#pg-integration`.

| case | scenario | expected |
|---|---|---|
| `positive_migration_idempotent` | `run_migrations` twice on a fresh DB | 2nd run no-ops; `_repo_graph_migrations` holds version 1 exactly once |
| `positive_shared_body_row_count` | write two snapshots of v1 | `graph_nodes` count = one body per key; `graph_node_versions` duplicated per snapshot |
| `negative_write_id_collision_leaves_building` | 2nd graph forces a new key onto a stored `node_id` | `Conflict`; snapshot still `building`; zero versions written (tx rolled back) |
| `adversarial_concurrent_begin_duplicate_identity` | two tasks race the same `(repo, sha, extractor_version)` | exactly one `Ok`, one `Conflict` (the identity unique) |
| `adversarial_cross_tenant_reads_return_nothing` | write under `ta`; every read verb + `repos`/`repo_get` under `tb` | `NotFound` / empty / `None`; no foreign row echoed |
| `corner_retention_sweeps_orphan_bodies` | keep newest of v1 (has `S`) + v2 (drops `S`) | snapshot 1 deleted; `S`'s orphaned body swept; still-referenced bodies kept |
| `adversarial_connect_bad_dsn_does_not_echo` | `connect` to a bad DSN carrying a password | `Err`; the error string contains neither the password nor the DSN |
| `corner_decode_corrupt_row_is_backend` | hand-corrupt a stored `lang`, then read | `Backend` fault (row rejected), never a panic |
| `negative_map_db_check_is_invalid` | a `commit_sha` that fails the DDL CHECK | `map_db` → `Invalid` |
| `boundary_diff_capped` | a diff exceeding `MAX_DIFF` (10 000) | lists truncated to 10 000, `truncated = true` |

Every R3 row also runs on `pg` with `after = assert_invariants` (edge endpoints present in-scope; each
`ready` snapshot's `node_count`/`edge_count` equals its stored rows).

## Harness legend

`repo_graph_conformance_suite!(<tier>, <harness expr>)` stamps every R3 row as
`<tier>::r3::<row>`. RK-01 instantiates `mem`; RK-02 adds `pg` (with `after = assert_invariants` /
`ignore =` for the live-Postgres gate) with zero row changes — the same rows are the contract both
stores meet. The `pg` tier additionally carries its own **P1** (in-gate, no DB) and **R4** (pg-only,
`#[ignore]`) tables above.

## Planned pipeline and knowledge cases (RK-03 onward)

**Specification, not an implementation/test result.** Existing R1–R4/P1 above retain their
identifiers and as-built scope. Every row below becomes a named table-driven `rstest` case;
where a cell lists boundary values, give each value its own named parameter row. Test bodies
assert the expected outcome and observable side effects, including absence of writes on failure.

Harnesses: **H** = hermetic memory/fake-clock/local-Git/scripted-provider suite;
**PG** = ignored live Postgres suite, serial at the suite level with explicit concurrency inside
race cases. R3 conformance is reused, not rewritten. Nix names and exact invocation live in
[10 — Integration and evaluation](10-integration-and-evaluation.md).

### E1 — deterministic extractors

RK-03 now has local executable cases in `agent-repo-graph/tests/extraction.rs` and
`extract/worker.rs`; acceptance evidence is in [PROGRESS](PROGRESS.md#2026-10-18--rk-03-extraction).
RK-06 still owns publication assertions for late/canceled extraction.

| Case ID | Class | Description/setup | Action | Expected outcome | Harness/gate | Increment |
|---|---|---|---|---|---|---|
| `positive_rust_cargo_docs` | Positive | Two-crate fixture, seam/impl, feature, linked docs, five test classes | Extract and compare expected nodes/edges | Correct qualified keys, cfg gates, doc links and test classes; heuristic tests labelled | H / repo-graph-rust | RK-03 |
| `positive_repeat_hash` | Positive | Identical fixture, reversed enumeration order | Extract twice | Identical sorted graph and pinned hash; no dependency download or compilation | H / repo-graph-rust | RK-03 |
| `negative_malformed_source` | Negative | Broken Rust/TOML beside valid files | Extract | Parse error counted, surviving facts valid, incomplete report; no invented relationships | H / repo-graph-rust | RK-03 |
| `boundary_extraction_caps` | Boundary | Files/bytes/nodes/edges at cap and cap+1 | Extract each variant | Exact cap accepted; excess skipped/truncated as specified; bounded memory | H / repo-graph-rust + bench/leak | RK-03 |
| `corner_empty_and_unresolved` | Corner | Empty repo; separately unresolved external imports and macro-generated items | Extract | Empty graph valid; unresolved coverage explicit; no inferred zero-callers claim | H / repo-graph-rust | RK-03 |
| `adversarial_escape_and_docs` | Adversarial | Escaping symlink, hostile path/doc instruction | Walk and extract | Outside file never read; instruction excluded; bounded diagnostic without hostile content | H / repo-graph-rust | RK-03 |
| `boundary_deadline_late_worker` | Boundary | Parser double held beyond deadline | Cancel, release worker, request another job | No late publication; worker slot held until completion; no accumulating workers | H / repo-graph-rust | RK-03/06 |
| `corner_canceled_worker_holds_admission` | Corner | Caller canceled while parser is blocked | Abort caller, request another job, then release parser | Cancellation signaled; admission retained until parser exits | H / repo-graph-rust | RK-03 |
| `positive_multiple_impl_blocks_and_specializations` | Positive | Repeated impl blocks and distinct generic self types | Extract, then edit second block | Blocks coalesce; specializations/methods remain distinct; body hash changes | H / repo-graph-rust | RK-03 |
| `corner_repeated_case_labels` | Corner | Two rstest rows share a named label | Extract | One named-case node; parent retains both rows | H / repo-graph-rust | RK-03 |
| `corner_anonymous_constants_and_file_cfg` | Corner | Multiple `_` constants in a feature-gated file | Extract | No anonymous symbol collision; file gate reaches function | H / repo-graph-rust | RK-03 |
| `positive_imported_trait_and_renamed_type` | Positive | Trait imported locally; self type renamed | Extract | Implements/impl-for edges reach workspace definitions | H / repo-graph-rust | RK-03 |
| `positive_confined_gitignore` | Positive | Local ignore rules hide directory and source | Extract | Ignored paths absent; no global rules consulted | H / repo-graph-rust | RK-03 |
| `adversarial_symlink_ignore_rules` | Adversarial | Root ignore file symlinks outside with `*` rule | Extract | Outside rules not read; source survives; report incomplete | H / repo-graph-rust | RK-03 |
| `boundary_individual_extractor_caps` | Boundary | Zero-edge builder cap followed by another extractor | Compose extractors | Cap failures reported; later extractor cannot relax cap | H / repo-graph-rust | RK-03 |
| `positive_comment_and_signature_hashes` | Positive | Doc-only edit followed by signature edit | Re-extract | Docs preserve symbol hashes but change file hash; signature changes signature hash | H / repo-graph-rust | RK-03 |
| `positive_nested_docs_preserve_symbol_hashes` | Positive | Edit field docs and function inner docs | Re-extract | Neither signature nor body hash changes for documentation edits | H / repo-graph-rust | RK-03 |
| `boundary_exact_graph_caps` | Boundary | Set caps to insertion counts, then one below | Re-extract | Exact cap preserves graph; each excess is explicitly truncated | H / repo-graph-rust | RK-03 |

### I1 — indexer publication and recovery

| Case ID | Class | Description/setup | Action | Expected outcome | Harness/gate | Increment |
|---|---|---|---|---|---|---|
| `positive_index_reuse_diff` | Positive | Git A then B; repeat same extractor identity | Index A twice and B; diff | Reused A snapshot id, expected B changes, reports preserved | H / repo-graph-index | RK-06 |
| `negative_publish_failure` | Negative | Store fails between graph writes and final state | Publish | No partial ready snapshot; failed status observable; ordinary readers unaffected | H / repo-graph-index | RK-06 |
| `boundary_retention` | Boundary | Existing retention clamps; keep=N and N+1 ready snapshots with shared nodes | Publish then sweep | Retain configured count; referenced shared nodes survive; report sweep failure separately | H / repo-graph-index | RK-06 |
| `corner_legacy_building` | Corner | Legacy committed building row with same identity | Index; operator recovers after stopping writer; retry | Initial Conflict; explicit failed transition audited; retry succeeds; no age takeover | H / repo-graph-index | RK-06 |
| `adversarial_wrong_root_scope` | Adversarial | Dirty worktree, incorrect registered remote, another tenant handle | Request indexing | Refuse misidentified checkout; no writes outside registered tenant/repo | H / repo-graph-index | RK-06 |
| `positive_atomic_race` | Positive | Two publishers for identical snapshot | Release simultaneous writes | One ready identity, other reuses it; no partially visible graph | H / repo-graph-index | RK-06 |

### I1 metadata and compatibility additions

| Case ID | Class | Description/setup | Action | Expected outcome | Harness/gate | Increment |
|---|---|---|---|---|---|---|
| `positive_report_roundtrip` | Positive | Report with parse errors/truncation in memory and Pg | Publish/read report | Counts/completeness preserved; partial graph never described as complete | H / repo-graph-index; PG / pg-integration | RK-06 |
| `negative_unknown_report_version` | Negative | Unsupported metadata envelope version | Decode snapshot report | Typed Backend error; no assumed-complete graph | H / repo-graph | RK-06 |
| `boundary_budget_identity` | Boundary | Same SHA/extractor code, larger count/byte cap | Index twice | New configuration fingerprint and snapshot identity; old incomplete snapshot not reused | H / repo-graph-index | RK-06 |
| `corner_legacy_metadata` | Corner | Existing array-only extractor metadata | Read with upgraded consumer | Legacy names decode; absent report explicitly unknown/incomplete | H / repo-graph | RK-06 |
| `adversarial_report_payload` | Adversarial | Huge/malformed report metadata and injected diagnostics | Decode/render | Bounded rejection; no panic or hostile diagnostic echoed | H / repo-graph | RK-06 |

### F1 — monitored-repository refresh

| Case ID | Class | Description/setup | Action | Expected outcome | Harness/gate | Increment |
|---|---|---|---|---|---|---|
| `positive_enroll_poll_mergebase` | Positive | Registered repo with A default and PR B | Enroll, poll new head, admit PR | Default head and merge-base jobs scheduled; exact SHAs pinned; review not blocked | H / repo-graph-index | RK-07 |
| `negative_unregistered_repo` | Negative | Fleet row has no matching repo registration | Poll | Recorded skip; no graph job; ordinary review continues | H / repo-graph-index | RK-07 |
| `boundary_refresh_queue` | Boundary | Queue capacity 16, one extraction active | Submit 16 distinct waiting jobs then one extra | Queue bounded; extra reported skipped and retried on next poll | H / repo-graph-index | RK-07 |
| `corner_repeat_and_cancel` | Corner | Repeated same-identity triggers; row deleted during job | Reconcile then cancel | One in-flight job; cleanup occurs; canceled result cannot publish | H / repo-graph-index | RK-07 |
| `adversarial_same_row_two_tenants` | Adversarial | Same row id and slug under two tenants | Review/index concurrently | Distinct cache/store views; no evidence or worktree crosses tenants | H / repo-graph-index | RK-07 |

### R5 — interactive graph tool

| Case ID | Class | Description/setup | Action | Expected outcome | Harness/gate | Increment |
|---|---|---|---|---|---|---|
| `positive_tool_cited_path` | Positive | Fixture with resolved path | Ask supported graph question | Bounded path and source SHA/node citations returned | H / review-repo-knowledge | RK-08 |
| `negative_tool_store_unavailable` | Negative | Store disconnected; separately unknown well-formed key | Query | Explicit unavailable for failure; successful empty result for unknown key | H / review-repo-knowledge | RK-08 |
| `boundary_tool_hops_bytes` | Boundary | Huge hop/count args, response at byte cap | Query | Existing graph clamps apply; whole records dropped with truncation metadata | H / review-repo-knowledge | RK-08 |
| `corner_tool_cycle_disabled` | Corner | Cyclic graph; separately disabled feature | Query/build tool list | Traversal terminates; disabled tool absent with prior behavior preserved | H / review-repo-knowledge | RK-08 |
| `adversarial_tool_scope_sql` | Adversarial | Foreign tenant field, SQL fragment, malformed key | Decode/query | Unknown fields/invalid input rejected; no SQL interpolation or foreign data | H / review-repo-knowledge | RK-08 |

### C1 — graph review and brief composition

| Case ID | Class | Description/setup | Action | Expected outcome | Harness/gate | Increment |
|---|---|---|---|---|---|---|
| `positive_exact_review_context` | Positive | PR touches known exported function | Run review with scripted model | Correct base SHA, touched keys, supported callers/tests and labelled provenance in prompt | H / review-repo-knowledge | RK-09 |
| `negative_missing_failed_graph` | Negative | No ready graph or store error | Run review | Existing collectors run; graph skipped/error status visible; no empty-success claim | H / review-repo-knowledge | RK-09 |
| `boundary_ancestor_and_brief` | Boundary | Ready ancestor at distance 50 then 51; brief at 4096 bytes then over | Select/render | Distance 50 labelled and usable historically; 51 skipped; whole cited entries fit budget | H / review-repo-knowledge | RK-09 |
| `corner_incomplete_unsupported` | Corner | Truncated Rust graph without resolved calls | Review | Completeness/unsupported labels; no claim that missing edges prove no impact | H / review-repo-knowledge | RK-09 |
| `adversarial_brief_injection` | Adversarial | Imported doc contains instructions and oversized strings | Render | Untrusted text screened/delimited; no instructions promoted; byte cap maintained | H / review-repo-knowledge | RK-09 |
| `positive_disabled_equivalence` | Positive | Same scripted review with feature disabled | Compare previous prompt/behavior | No graph queries/context; baseline behavior preserved | H / review-repo-knowledge | RK-09 |

### K1 — knowledge store and lifecycle

Run shared K1 rows against memory and Postgres; the PG stamp belongs to K5 below.

| Case ID | Class | Description/setup | Action | Expected outcome | Harness/gate | Increment |
|---|---|---|---|---|---|---|
| `positive_knowledge_lifecycle` | Positive | Valid evidence and authorized actor | Propose, confirm, inspect, reject another candidate, supersede confirmed record | Expected states/events/revisions; old text retained; only replacement eligible | H / repo-knowledge; PG / pg-integration | RK-19 |
| `negative_transition_conflict` | Negative | Rejected record, stale revision, or unconfirmed replacement | Confirm/supersede | Typed Conflict; no event or partial state mutation | H / repo-knowledge; PG / pg-integration | RK-19 |
| `boundary_knowledge_limits` | Boundary | Each title/body/evidence/link/excerpt/reason limit and limit+1 | Propose/transition/list | Exact allowed; over-limit writes rejected; list clamp enforced | H / repo-knowledge; PG / pg-integration | RK-19 |
| `corner_duplicate_and_self_replace` | Corner | Identical proposal twice, including after rejection; self-supersession | Propose/supersede | Existing id returned without resurrecting state; self-replacement rejected | H / repo-knowledge; PG / pg-integration | RK-19 |
| `adversarial_foreign_evidence` | Adversarial | Cross-tenant/repo evidence, actor spoof, unknown node key | Propose/confirm | Refused; no foreign row/text returned; no confirmation event | H / repo-knowledge; PG / pg-integration | RK-19 |

### K2 — evidence and freshness

| Case ID | Class | Description/setup | Action | Expected outcome | Harness/gate | Increment |
|---|---|---|---|---|---|---|
| `positive_unchanged_new_commit` | Positive | New commit with unchanged relevant code/docs | Check freshness | Current; commit change alone does not invalidate | H / repo-knowledge | RK-19/20 |
| `negative_missing_source` | Negative | Required feedback unavailable or nonexistent doc citation | Confirm | Unverifiable/rejected confirmation; existing historical record remains inspectable | H / repo-knowledge | RK-20 |
| `boundary_evidence_excerpt` | Boundary | Multibyte UTF-8 excerpts at per-item/total limits and one byte over | Validate | Byte limits enforced without broken UTF-8; oversized proposal rejected | H / repo-knowledge | RK-19/20 |
| `corner_rename_doc_retention` | Corner | Rename/delete symbol, change doc only, then prune old snapshot | Check/retrieve | Stale/unverifiable as appropriate; no automatic remapping; audit history survives | H / repo-knowledge | RK-19/20 |
| `adversarial_poisoned_evidence` | Adversarial | Instruction text, secret-bearing URL, mismatched content hash | Validate/render | Unsafe content rejected/redacted; hash mismatch blocks confirmation; no secret in diagnostic | H / repo-knowledge | RK-20 |

### K3 — proposals and operator CLI

| Case ID | Class | Description/setup | Action | Expected outcome | Harness/gate | Increment |
|---|---|---|---|---|---|---|
| `positive_operator_confirmation` | Positive | Valid proposal JSON and authenticated operator | Invoke CLI confirmation | Correct tenant/actor recorded; explicit successful revision printed | H / repo-knowledge + cli-help | RK-20 |
| `negative_model_confirm` | Negative | Model attempts lifecycle mutation or JSON sets confirmed state | Decode/tool dispatch | No mutation tool; unknown proposal fields refused; no durable promotion | H / repo-knowledge | RK-20 |
| `boundary_cli_read_limit` | Boundary | List limits 0, 20, 100, 101 | List | Documented clamp/default; bounded output and stable order | H / repo-knowledge | RK-20 |
| `corner_addressed_and_merge` | Corner | Finding becomes addressed; review posts; PR merges; repeated callback | Generate proposal | At most identical candidate id reused; no auto-confirmation | H / repo-knowledge | RK-20 |
| `adversarial_cli_identity` | Adversarial | Proposal carries tenant/actor override or unsafe repo slug | Invoke CLI | Refused before state write; trusted caller identity not overridden | H / repo-knowledge | RK-20 |
| `negative_proposal_provider_failure` | Negative | Scripted provider times out/returns malformed JSON | Process feedback | Bounded failure recorded; review/draft/post state unchanged | H / repo-knowledge | RK-20 |

### K4 — confirmed guidance in context

| Case ID | Class | Description/setup | Action | Expected outcome | Harness/gate | Increment |
|---|---|---|---|---|---|---|
| `positive_confirmed_selection` | Positive | Confirmed current direct and two-hop matches | Build review/Implement briefs | Deterministic rank, citations/status and selected snapshot included | H / repo-knowledge | RK-21 |
| `negative_ineligible_guidance` | Negative | Candidate/rejected/superseded/stale/unverifiable records | Build brief | None rendered as guidance; omission counts without content leakage | H / repo-knowledge | RK-21 |
| `boundary_guidance_budget` | Boundary | 5 then 6 eligible records; 2048-byte sub-budget; configured zero | Render | At most five; whole entries fit within existing 4096/6144 totals; zero injects nothing | H / repo-knowledge | RK-21 |
| `corner_ancestor_only_disabled` | Corner | Only ancestor snapshot; separately knowledge disabled | Build brief | No unverified current guidance; disabled path makes no knowledge queries | H / repo-knowledge | RK-21 |
| `adversarial_guidance_override` | Adversarial | Confirmed text still contains policy-like language or foreign citation | Retrieve/render | Repository evidence cannot change policy; foreign evidence refused; untrusted section retained | H / repo-knowledge | RK-21 |

### I2 / K5 — live integration scenarios

| Case ID | Class | Description/setup | Action | Expected outcome | Harness/gate | Increment |
|---|---|---|---|---|---|---|
| `positive_pg_pipeline_restart` (I2) | Positive | Real Git A/B/C, Pg, fake forge and scripted provider | Index/review; reconnect; review again | Persisted graph reused and correct cited context survives restart | PG / pg-integration | RK-06/09 |
| `negative_pg_disconnect` (I2) | Negative | Connection dropped mid-publication, then DB unavailable during review | Retry/review | No committed partial ready state; review continues with visible graph error | PG / pg-integration | RK-06/09 |
| `boundary_pg_retention_read` (I2) | Boundary | Sweep at keep limit while brief reads several queries | Interleave with barriers | Reader sees consistent snapshot; shared bodies retained | PG / pg-integration | RK-06/09 |
| `corner_pg_concurrent_publish` (I2) | Corner | Two pools publish same identity | Race and reconnect | One ready identity reused; no duplicates or stranded committed building row | PG / pg-integration | RK-06 |
| `adversarial_pg_fleet_tenants` (I2) | Adversarial | Same fleet row id in two tenants | Index and review both | No graph/context/workspace sharing across tenants | PG / pg-integration | RK-07/09 |
| `positive_pg_knowledge_migrate` (K5) | Positive | Fresh DB and DB with migration 0001; later migrations applied in order | Apply/reapply, confirm, reconnect/retrieve | One ledger entry per migration; old graph intact; confirmed knowledge persists | PG / pg-integration | RK-19/21 |
| `negative_pg_confirmation_race` (K5) | Negative | Two pools confirm/reject same expected revision | Race transitions | Exactly one succeeds; other Conflict; exactly one transition event | PG / pg-integration | RK-19/20 |
| `boundary_pg_evidence_retention` (K5) | Boundary | Old snapshot reaches retention limit; evidence history disappears | Sweep then inspect/confirm | Existing audit preserved; missing required evidence blocks new confirmation | PG / pg-integration | RK-19/21 |
| `corner_pg_supersession_restart` (K5) | Corner | Changed evidence, confirmed replacement | Supersede, restart, retrieve | Old remains historical; only current replacement injected | PG / pg-integration | RK-20/21 |
| `adversarial_pg_replacement_scope` (K5) | Adversarial | Confirmed replacement from another repo/tenant | Supersede | Rejected atomically; neither record changed | PG / pg-integration | RK-19/20 |
| `negative_required_pg_skipped` (I2) | Negative | Required flag set; missing runtime/DSN or zero selected tests | Run harness | Exit 1 and failed/incomplete artifact; never acceptance PASS | H runner / pg-integration | RK-06 |
| `corner_optional_pg_skipped` (I2) | Corner | Required flag unset and runtime unavailable | Run aggregate | Explicit skipped result; not counted as executed success | H runner / pg-integration | RK-06 |

### Q1 — evaluation runner and measurement integrity

| Case ID | Class | Description/setup | Action | Expected outcome | Harness/gate | Increment |
|---|---|---|---|---|---|---|
| `positive_paired_manifest` | Positive | Frozen corpus and scripted model | Run A/B then A/B/C | Same PRs/budgets/settings; per-arm artifacts and indexing costs separated | H / review-repo-knowledge, repo-knowledge | RK-09/21 |
| `negative_incomplete_arm` | Negative | Model failure or missing adjudication | Finish run | Incomplete/error reported; no claimed quality acceptance | H / review-repo-knowledge | RK-09 |
| `boundary_acceptance_threshold` | Boundary | Exact 10% token gain/latency regression, then beyond allowed regression | Compute report | Thresholds applied as documented; false-positive increase blocks acceptance | H / review-repo-knowledge | RK-09 |
| `corner_no_findings` | Corner | Empty valid/false finding sets | Compute metrics | No division-by-zero; undefined ratios labelled; no invented quality gain | H / review-repo-knowledge | RK-09 |
| `adversarial_heldout_leakage` | Adversarial | Lesson corpus includes evaluated PR outcome | Validate manifest | Run rejected as contaminated before scoring | H / repo-knowledge | RK-21 |
