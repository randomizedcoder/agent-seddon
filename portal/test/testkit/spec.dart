/// The test-spec row — the executable form of the design's per-page spec tables
/// (docs/design/portal-gui-testing/02-test-spec.md). Every widget test is a row;
/// adding a test is adding a row, and adding an element is adding its `Key` plus
/// its rows. The completeness critic (`meta/coverage_test.dart`) reads the
/// [elementId]s across all page specs to prove no keyed element is untested.
library;

/// The five case classes, matching the Rust `rstest` `#[case::<prefix>_…]`
/// convention and CLAUDE.md's four-class rule (+ mandatory `adversarial_`).
enum CaseClass { positive, negative, boundary, corner, adversarial }

extension CaseClassName on CaseClass {
  String get prefix => switch (this) {
        CaseClass.positive => 'positive',
        CaseClass.negative => 'negative',
        CaseClass.boundary => 'boundary',
        CaseClass.corner => 'corner',
        CaseClass.adversarial => 'adversarial',
      };
}

/// One spec row. [elementId] is the widget-key **stem** it exercises (e.g.
/// `prompts.save`, or the family stem `prompts.list.item` for interpolated
/// per-item keys) — the join key across app code, spec, report, and perf table.
class SpecRow {
  const SpecRow({
    required this.elementId,
    required this.caseClass,
    required this.name,
    required this.description,
    this.expectedRpc = 'local',
  });

  final String elementId;
  final CaseClass caseClass;

  /// Short case name; the full test label is `<prefix>_<name>`.
  final String name;
  final String description;

  /// `agent.v1.<Service>/<Method>` the row expects to fire, or `local` for a
  /// browser-only action (no backend RPC).
  final String expectedRpc;

  String get label => '${caseClass.prefix}_$name';
}

/// One page's spec: its element ids and rows. The critic requires that every
/// `Key` stem scanned from the page's source appears as some row's [elementId].
class PageSpec {
  const PageSpec(this.page, this.rows);

  /// The `<page>` prefix of this page's keys (e.g. `prompts`, `launch`).
  final String page;
  final List<SpecRow> rows;

  /// The distinct element ids this page's rows cover.
  Set<String> get coveredElements => {for (final r in rows) r.elementId};
}
