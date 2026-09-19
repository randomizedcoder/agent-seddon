import '../testkit/spec.dart';

/// Graph page spec (design 02) — the largest page (~32 key stems). Covers the
/// page-init states (loading / seeded library / gateway-offline banner), the
/// local library + node/edge editing (which persist to the in-memory storage stub
/// on the VM, so outcomes are asserted via the widget tree, not RPCs), the server
/// actions (`Validate` / `Put` / `DescribeNodeTypes`), and the mandatory
/// `adversarial_` rows (a hostile raw-JSON params payload; an inert import).
///
/// Every `Key(...)` stem in `graph_page.dart` has a `positive_` row here, so the
/// completeness critic passes for this page once it is tabled.
const graphSpec = PageSpec('graph', [
  // ── page init ──────────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'graph.library.item',
    caseClass: CaseClass.positive,
    name: 'library_lists_seeded',
    description: 'a seeded library renders one tile per entry; DescribeNodeTypes fires',
    expectedRpc: 'agent.v1.GraphService/DescribeNodeTypes',
  ),
  SpecRow(
    elementId: 'graph.library.item',
    caseClass: CaseClass.corner,
    name: 'empty_seeds_examples',
    description: 'an empty library auto-seeds the shipped example graphs',
    expectedRpc: 'agent.v1.GraphService/DescribeNodeTypes',
  ),
  SpecRow(
    elementId: 'graph.library.item',
    caseClass: CaseClass.boundary,
    name: 'loading_shows_spinner',
    description: 'before the init RPCs resolve, a progress spinner is shown',
    expectedRpc: 'agent.v1.GraphService/DescribeNodeTypes',
  ),
  SpecRow(
    elementId: 'graph.retry',
    caseClass: CaseClass.negative,
    name: 'offline_shows_banner',
    description: 'a failed DescribeNodeTypes surfaces the offline retry banner',
    expectedRpc: 'agent.v1.GraphService/DescribeNodeTypes',
  ),
  SpecRow(
    elementId: 'graph.retry',
    caseClass: CaseClass.positive,
    name: 'retry_reprobes',
    description: 'Retry re-runs DescribeNodeTypes and clears the banner',
    expectedRpc: 'agent.v1.GraphService/DescribeNodeTypes',
  ),

  // ── library actions ─────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'graph.new',
    caseClass: CaseClass.positive,
    name: 'new_adds_entry',
    description: 'New opens the name dialog; OK adds a fresh entry',
  ),
  SpecRow(
    elementId: 'graph.name.field',
    caseClass: CaseClass.positive,
    name: 'name_field_accepts_text',
    description: 'the name dialog field accepts typed input',
  ),
  SpecRow(
    elementId: 'graph.name.ok',
    caseClass: CaseClass.positive,
    name: 'name_ok_confirms',
    description: 'OK in the name dialog confirms the entered name',
  ),
  SpecRow(
    elementId: 'graph.name.cancel',
    caseClass: CaseClass.positive,
    name: 'name_cancel_dismisses',
    description: 'Cancel dismisses the name dialog and adds nothing',
  ),
  SpecRow(
    elementId: 'graph.import',
    caseClass: CaseClass.positive,
    name: 'import_button_present',
    description: 'the Import control exists and taps without a crash',
  ),
  SpecRow(
    elementId: 'graph.import',
    caseClass: CaseClass.adversarial,
    name: 'import_bad_json_safe',
    description: 'with no file selected (VM stub) Import is an inert no-op — no crash, no mutation, no RPC',
  ),
  SpecRow(
    elementId: 'graph.rename',
    caseClass: CaseClass.positive,
    name: 'rename_updates_name',
    description: 'Rename opens the name dialog and renames the selected entry',
  ),
  SpecRow(
    elementId: 'graph.duplicate',
    caseClass: CaseClass.positive,
    name: 'duplicate_copies_entry',
    description: 'Duplicate adds a "<name> copy" entry',
  ),
  SpecRow(
    elementId: 'graph.export',
    caseClass: CaseClass.positive,
    name: 'export_noop_safe',
    description: 'Export (inert download on the VM) does not crash',
  ),
  SpecRow(
    elementId: 'graph.delete',
    caseClass: CaseClass.positive,
    name: 'delete_removes_entry',
    description: 'Delete removes the selected entry from the library',
  ),

  // ── server actions ──────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'graph.validate',
    caseClass: CaseClass.positive,
    name: 'validate_shows_issues',
    description: 'Validate fires Validate {graph} and shows the issues dialog',
    expectedRpc: 'agent.v1.GraphService/Validate',
  ),
  SpecRow(
    elementId: 'graph.validate.issues.close',
    caseClass: CaseClass.positive,
    name: 'close_issues_dialog',
    description: 'Close dismisses the validation-issues dialog',
  ),
  SpecRow(
    elementId: 'graph.setActive',
    caseClass: CaseClass.positive,
    name: 'set_active_puts',
    description: 'Set active fires Put {graph}',
    expectedRpc: 'agent.v1.GraphService/Put',
  ),
  SpecRow(
    elementId: 'graph.setActive',
    caseClass: CaseClass.negative,
    name: 'set_active_rejected_snacks',
    description: 'a rejected Put shows a failure SnackBar, no crash',
    expectedRpc: 'agent.v1.GraphService/Put',
  ),

  // ── node editing ────────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'graph.node',
    caseClass: CaseClass.positive,
    name: 'node_tiles_render',
    description: 'each node in the selected graph renders a tile',
  ),
  SpecRow(
    elementId: 'graph.node.add',
    caseClass: CaseClass.positive,
    name: 'add_node_appends',
    description: 'Add node opens the dialog; Add appends the node',
  ),
  SpecRow(
    elementId: 'graph.node.add',
    caseClass: CaseClass.negative,
    name: 'add_node_blocked_offline',
    description: 'with node-types unfetched, Add node is blocked with a SnackBar',
  ),
  SpecRow(
    elementId: 'graph.node.add.id',
    caseClass: CaseClass.positive,
    name: 'add_node_id_field',
    description: 'the add-node dialog exposes the id field',
  ),
  SpecRow(
    elementId: 'graph.node.add.type',
    caseClass: CaseClass.positive,
    name: 'add_node_type_dropdown',
    description: 'the add-node dialog exposes the type dropdown',
  ),
  SpecRow(
    elementId: 'graph.node.add.confirm',
    caseClass: CaseClass.corner,
    name: 'add_node_confirm_requires_id',
    description: 'confirming with an empty id shows a validation error, no add',
  ),
  SpecRow(
    elementId: 'graph.node.add.cancel',
    caseClass: CaseClass.positive,
    name: 'add_node_cancel_dismisses',
    description: 'Cancel dismisses the add-node dialog without adding',
  ),
  SpecRow(
    elementId: 'graph.node.remove',
    caseClass: CaseClass.positive,
    name: 'remove_node_and_dangling_edges',
    description: 'removing a node drops it and its dangling edges',
  ),
  SpecRow(
    elementId: 'graph.node.params.toggle',
    caseClass: CaseClass.positive,
    name: 'toggle_form_to_raw',
    description: 'the Form/Raw toggle switches a simple-schema node to raw JSON',
  ),
  SpecRow(
    elementId: 'graph.node.params.field',
    caseClass: CaseClass.positive,
    name: 'param_field_edits',
    description: 'a schema-driven param field accepts input',
  ),
  SpecRow(
    elementId: 'graph.node.params.raw',
    caseClass: CaseClass.positive,
    name: 'raw_editor_for_schemaless',
    description: 'a node with no simple schema renders the raw-JSON editor',
  ),
  SpecRow(
    elementId: 'graph.node.params.raw',
    caseClass: CaseClass.adversarial,
    name: 'raw_giant_invalid_json',
    description: 'a huge / invalid raw-JSON params payload is rejected without a crash',
  ),

  // ── edge editing ────────────────────────────────────────────────────────────
  SpecRow(
    elementId: 'graph.edge',
    caseClass: CaseClass.positive,
    name: 'edge_tiles_render',
    description: 'each edge in the selected graph renders a tile',
  ),
  SpecRow(
    elementId: 'graph.edge.add',
    caseClass: CaseClass.positive,
    name: 'add_edge_appends',
    description: 'Add edge opens the dialog; Add appends the edge',
  ),
  SpecRow(
    elementId: 'graph.edge.add.from',
    caseClass: CaseClass.positive,
    name: 'add_edge_from_field',
    description: 'the add-edge dialog exposes the from field',
  ),
  SpecRow(
    elementId: 'graph.edge.add.to',
    caseClass: CaseClass.positive,
    name: 'add_edge_to_field',
    description: 'the add-edge dialog exposes the to field',
  ),
  SpecRow(
    elementId: 'graph.edge.add.kind',
    caseClass: CaseClass.positive,
    name: 'add_edge_kind_dropdown',
    description: 'the add-edge dialog exposes the kind dropdown',
  ),
  SpecRow(
    elementId: 'graph.edge.add.confirm',
    caseClass: CaseClass.corner,
    name: 'add_edge_confirm_requires_endpoints',
    description: 'confirming with empty endpoints adds nothing',
  ),
  SpecRow(
    elementId: 'graph.edge.add.cancel',
    caseClass: CaseClass.positive,
    name: 'add_edge_cancel_dismisses',
    description: 'Cancel dismisses the add-edge dialog without adding',
  ),
  SpecRow(
    elementId: 'graph.edge.remove',
    caseClass: CaseClass.positive,
    name: 'remove_edge',
    description: 'removing an edge drops its tile',
  ),
]);
