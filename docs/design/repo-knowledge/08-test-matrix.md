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
| `negative_reject` (unknown prefix, unknown rust kind, empty, prefix-only `rust:fn:`, `rust:`, a bare word) | | `Invalid`; the message names the rule, not the input |
| `corner_go_interface_is_trait` / `corner_go_func_is_fn` | `go:interface:p.I`, `go:func:p.F` | kinds `trait` / `fn` |
| `boundary_key_512` / `boundary_key_513` | | ok / `TooLong("node_key")` |
| `adversarial_charset` (whitespace, tab, control char, non-ASCII, bidi override, bad dup suffix `@zz`, double `@`, uppercase dup) | | rejected; the message echoes none of the input |
| `adversarial_file_path` (`file:../x`, absolute, backslash, `./`, `//`, `doc:a/../b`) and `repo_relative_rows` | | rejected |
| `adversarial_constructor_segment_colon` / `_segment_whitespace` / `_crate_dash` (`agent-core` must be `agent_core`) / `_rust_item_bad_kind` | | constructor returns `Invalid` |
| `positive_id_known_vector` (`file:a`), `positive_id_deterministic`, `corner_id_high_bit_negative` | | the sha256[..8] as i64; equal across calls; a top-bit key gives a negative id |
| `positive_tokens` (snake, camel, acronym `HTTPServer`→`http,server`, mixed `PgDigests`, dotted), `corner_tokens_dedup`, `boundary_tokens_16` (17 parts ⇒ 16), `adversarial_tokens_huge_name` (10 KiB ⇒ empty), `adversarial_tokens_unicode` (non-ASCII dropped) | | |

## R2 — `GraphBuilder` + validation

Pure, `agent_core::repo_graph::builder`.

| case | input | expected |
|---|---|---|
| `positive_sorted_output` | nodes / edges inserted out of order | `nodes()` by key, `edges()` by `(kind, src, dst)` |
| `positive_hash_deterministic` | the same items in two orders | equal `graph_hash` |
| `positive_hash_tracks_body` / `_sig`, `corner_hash_ignores_attrs_and_lines` | one field changed | hash changes / hash unchanged |
| `positive_cfg_dup_suffixed` | same key, cfg `["feature=a"]` then `["feature=b"]` | second `Suffixed(key@sha8)`, `attrs.dup = true`, `attrs.cfg` recorded |
| `negative_dup_same_cfg_fails` | same key, same cfg | `finish` ⇒ `BuildError::DuplicateKey` |
| `negative_id_collision_fails` (`with_id_fn(|_| NodeId(7))`) | two keys, one id | `BuildError::IdCollision` |
| `negative_dangling_edge_dropped` | edge to a key never added | edge absent, `dropped_edges = 1` |
| `corner_edge_before_node_kept` | edge, then both nodes | edge present |
| `corner_empty_graph` | nothing | `finish` ok, the constant empty hash, zero counts |
| `boundary_max_nodes` / `_max_edges` (caps 3 / 3) | 4 inserts | 3 kept, `truncated`, one dropped and counted |
| `boundary_name_256` / `_257`, `_qualifier_512` / `_513`, `_attrs_4096` / `_4097`, `_edge_attrs_1024` / `_1025` | | kept / `Dropped(DropReason::…)` naming the field |
| `adversarial_file_traversal` / `_file_absolute` / `_file_control` / `_attrs_not_object` / `_weight_nan` / `_weight_negative` / `_lines_reversed` | | node dropped / dropped / dropped / dropped / weight `1.0` / `1.0` / `line_end = line_start` |
| `adversarial_drop_reason_never_echoes` | a 4 KiB hostile name | `DropReason` `Display` contains no byte of it |

## R3 — store conformance

`MemRepoGraph` now; `PgRepoGraph` reruns every row through `repo_graph_conformance_suite!` in
RK-02. `agent_testkit::repo_graph::conformance`.

| case | scenario | expected |
|---|---|---|
| `positive_repo_put_get` / `_upsert` / `positive_repos_sorted` / `negative_repo_get_unknown` | | ids stable across upsert, fields updated, `None` |
| `adversarial_repo_slug_unsafe` (`../x`, `a b`, 129 chars, empty), `_repo_profile_huge`, `_repo_remote_control_char` | | `Invalid` / `TooLong` naming the field |
| `adversarial_repo_cross_tenant` | put under `ta`, get under `tb` | `None`; `repos()` under `tb` empty |
| `positive_snapshot_lifecycle` | begin → write v1 → finish Ready | `Snapshot` has hash, counts, `ready`, `duration_ms` |
| `positive_snapshot_find_by_sha` / `_latest_skips_failed_and_building` / `_snapshots_newest_first_limited` | | |
| `positive_bodies_shared` | two snapshots of v1 | same `NodeId`s, `nodes_by_key` identical on both |
| `positive_snapshot_diff` | v1 vs v2 | `added=[delta]`, `removed=[S,…]`, `sig_changed=[gamma]`, `body_changed=[beta]`, edges as built |
| `positive_retention` | 4 ready + 1 failed, keep 2 | returns 3; the two newest readable, the rest `NotFound` |
| `negative_begin_unknown_repo` / `_duplicate_identity` / `corner_begin_replaces_failed` / `negative_begin_bad_sha` / `adversarial_begin_extractor_name` | | `NotFound` / `Conflict` / new id / `Invalid` |
| `negative_write_after_finish` / `_finish_twice` / `_finish_unknown` / `corner_write_empty_graph` / `negative_write_id_collision` | | `Conflict` / `Conflict` / `NotFound` / ok, zero counts / `Conflict`, snapshot still `building`, nothing written |
| `adversarial_snapshot_cross_repo` / `_cross_tenant` / `_diff_cross_repo` | mismatched scope pairs | `NotFound` |
| `positive_nodes_by_key` / `_by_file` / `_by_name_kind_filter` / `negative_unknown_key_empty` / `adversarial_name_huge` / `_name_whitespace` | | rows / empty, no error |
| `positive_neighbors_out_calls_2_hops` / `_neighbors_in` / `corner_neighbors_cycle_terminates` / `boundary_hops_clamped` / `_cap_clamped` / `_keys_64` | | depths and clamps as specified |
| `positive_blast_radius` / `_tests_covering_via` / `_path_between` / `negative_path_none` / `corner_path_cycle_guard` / `boundary_paths_clamped` | | |
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
