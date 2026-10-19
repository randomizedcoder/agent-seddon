import 'dart:convert' show utf8;

import 'package:fixnum/fixnum.dart';
import 'package:flutter/material.dart';

import '../gen/agent/v1/forge_registry.pb.dart';
import '../gen/agent/v1/review_fleet.pb.dart';

/// The longest roster id / user / repo slug the agent accepts
/// (`agent_core::MAX_SEGMENT_LEN`).
const maxSessionSegmentLen = 128;

/// The longest free-text roster field (`agent_core::MAX_FLEET_NAME_LEN`, bytes).
const maxSessionTextBytes = 256;

/// The poll interval bounds the agent clamps into (`MIN_/MAX_FLEET_POLL_SECS`);
/// `0` means the agent's default.
const minPollSecs = 30;
const maxPollSecs = 86400;

/// A roster segment the agent's `safe_segment` would accept, checked a little
/// stricter (no leading `.`, no `..`): ASCII letters, digits, `.`, `_`, `-`.
/// Only to explain early — the agent validates again.
bool validSessionSegment(String s) =>
    s.isNotEmpty &&
    s.length <= maxSessionSegmentLen &&
    RegExp(r'^[A-Za-z0-9._-]+$').hasMatch(s) &&
    !s.startsWith('.') &&
    !s.startsWith('-') &&
    !s.contains('..');

/// The roster slug for what the operator typed as the repo: `owner/name` (or
/// GitLab's `group/subgroup/name`) becomes `owner__name`; an already-encoded
/// slug is kept. With [ownerName] (GitHub, Gitea, Bitbucket) the slug must split
/// into exactly an owner and a name, as the agent's `decode_owner_name` does.
/// Null when the agent would refuse it.
String? repoSlug(String input, {required bool ownerName}) {
  final parts = input.trim().split('/');
  if (parts.any((p) => !validSessionSegment(p))) return null;
  final slug = parts.join('__');
  if (!validSessionSegment(slug)) return null;
  if (ownerName) {
    final i = slug.indexOf('__');
    if (i <= 0 || i + 2 >= slug.length) return null;
    if (!validSessionSegment(slug.substring(0, i)) ||
        !validSessionSegment(slug.substring(i + 2))) {
      return null;
    }
  }
  return slug;
}

/// What the session dialog collected, as typed.
class SessionDraft {
  const SessionDraft({
    this.id = '',
    this.forgeId = '',
    this.repo = '',
    this.user = '',
    this.skill = '',
    this.poll = '',
    this.enabled = false,
  });

  final String id;
  final String forgeId;
  final String repo;
  final String user;
  final String skill;
  final String poll;
  final bool enabled;
}

/// Turn a [draft] into the row to `Put`, or explain why not.
///
/// [base] is the row being edited (null for a new one): every field the dialog
/// does not show — Slack channels, transport card, timestamps — is carried over
/// from it, so an edit never blanks them, and its id and `user` are kept.
///
/// A row's `user` is its **tenant** to the fleet: the owning org its workspace,
/// metrics and `ghapp:` mint gate are keyed by. So a new row signed in as
/// [tenant] belongs to that tenant, whatever was typed; only with no signed-in
/// tenant (a single-operator agent) is it taken from the form, defaulting to
/// the repository owner. A new row must name one of [cards]
/// (null when the cards could not be listed); its id must not be in
/// [takenIds], because `Put` is an upsert and would overwrite that session.
/// With a forge card chosen, the inline `backend`/`base_url`/`token_ref` are
/// cleared — the card supersedes them, and a stale inline reference should not
/// linger. A row edited with no card keeps its inline forge settings.
({FleetSession? session, String? error}) buildSession(
  SessionDraft draft, {
  FleetSession? base,
  required List<ForgeCard>? cards,
  required Set<String> takenIds,
  String? tenant,
}) {
  ({FleetSession? session, String? error}) no(String why) =>
      (session: null, error: why);

  final isNew = base == null;
  final id = isNew ? draft.id.trim() : base.id;
  if (isNew) {
    if (!validSessionSegment(id)) {
      return no('Session id: letters, digits, ".", "_" or "-" (no leading "." '
          'or "-", no "..", at most $maxSessionSegmentLen).');
    }
    if (takenIds.contains(id)) return no('A session "$id" already exists.');
  }

  final forgeId = draft.forgeId.trim();
  final card = cards?.where((c) => c.id == forgeId).firstOrNull;
  final keepsOwnForge = !isNew && forgeId == base.forgeId;
  if (forgeId.isEmpty) {
    if (isNew || base.forgeId.isNotEmpty) return no('Choose a forge card.');
  } else if (!validSessionSegment(forgeId)) {
    return no('That forge card id is not valid.');
  } else if (card == null && !keepsOwnForge) {
    return no('Forge card "$forgeId" was not found.');
  }

  final ownerName = card == null || card.repoEncoding != 'path';
  final repo = repoSlug(draft.repo, ownerName: ownerName);
  if (repo == null) {
    return no(ownerName
        ? 'Repository: owner/name, e.g. acme/widgets.'
        : 'Repository: group/name or group/subgroup/name.');
  }

  var user = draft.user.trim();
  if (!isNew) {
    user = base.user;
  } else if (tenant != null && tenant.isNotEmpty) {
    user = tenant;
  } else if (user.isEmpty) {
    user = repo.split('__').first;
  }
  if (!validSessionSegment(user)) {
    return no('Workspace owner: letters, digits, ".", "_" or "-".');
  }

  final skill = draft.skill.trim();
  if (utf8.encode(skill).length > maxSessionTextBytes ||
      skill.runes.any((r) => r < 0x20 || r == 0x7f)) {
    return no('Skill: at most $maxSessionTextBytes bytes, no control characters.');
  }

  final pollText = draft.poll.trim();
  var poll = 0;
  if (pollText.isNotEmpty) {
    final p = RegExp(r'^[0-9]{1,6}$').hasMatch(pollText) ? int.parse(pollText) : -1;
    if (p != 0 && (p < minPollSecs || p > maxPollSecs)) {
      return no('Poll interval: $minPollSecs–$maxPollSecs seconds, or blank '
          'for the default.');
    }
    poll = p;
  }

  final s = (base?.deepCopy() ?? FleetSession())
    ..id = id
    ..forgeId = forgeId
    ..repo = repo
    ..user = user
    ..skill = skill
    ..pollSecs = Int64(poll)
    ..enabled = draft.enabled;
  if (forgeId.isNotEmpty) {
    s
      ..backend = ''
      ..baseUrl = ''
      ..tokenRef = '';
  }
  return (session: s, error: null);
}

/// Creates or edits one fleet session. Pops the row to `Put`, or null.
class FleetSessionDialog extends StatefulWidget {
  const FleetSessionDialog({
    super.key,
    this.base,
    required this.cards,
    required this.cardsError,
    required this.takenIds,
    this.tenant,
  });

  /// The row being edited; null for a new one.
  final FleetSession? base;

  /// The forge cards to pick from; null when they could not be listed.
  final List<ForgeCard>? cards;
  final String? cardsError;
  final Set<String> takenIds;

  /// The signed-in tenant, which owns a new session; null when signed out.
  final String? tenant;

  @override
  State<FleetSessionDialog> createState() => _FleetSessionDialogState();
}

class _FleetSessionDialogState extends State<FleetSessionDialog> {
  late final _id = TextEditingController(text: widget.base?.id ?? '');
  late final _repo = TextEditingController(text: widget.base?.repo ?? '');
  late final _user = TextEditingController(
      text: widget.base?.user ?? widget.tenant ?? '');
  late final _skill = TextEditingController(text: widget.base?.skill ?? '');
  late final _poll = TextEditingController(
      text: (widget.base?.pollSecs.toInt() ?? 0) == 0
          ? ''
          : '${widget.base!.pollSecs}');
  late String _forgeId = widget.base?.forgeId ??
      (widget.cards?.where((c) => c.enabled).firstOrNull?.id ?? '');
  late bool _enabled = widget.base?.enabled ?? false;
  String? _error;

  @override
  void dispose() {
    for (final c in [_id, _repo, _user, _skill, _poll]) {
      c.dispose();
    }
    super.dispose();
  }

  void _submit() {
    final r = buildSession(
      SessionDraft(
        id: _id.text,
        forgeId: _forgeId,
        repo: _repo.text,
        user: _user.text,
        skill: _skill.text,
        poll: _poll.text,
        enabled: _enabled,
      ),
      base: widget.base,
      cards: widget.cards,
      takenIds: widget.takenIds,
      tenant: widget.tenant,
    );
    if (r.session == null) {
      setState(() => _error = r.error);
      return;
    }
    Navigator.pop(context, r.session);
  }

  /// The picker's choices: every listed card, the row's own card even if it is
  /// gone (so an edit doesn't silently move it), and — only for a row that
  /// already uses inline settings — keeping those.
  List<DropdownMenuItem<String>> _forgeItems() {
    final base = widget.base;
    final cards = widget.cards ?? const <ForgeCard>[];
    return [
      if (base != null && base.forgeId.isEmpty)
        const DropdownMenuItem(value: '', child: Text('Inline settings (from config)')),
      for (final c in cards)
        DropdownMenuItem(
          value: c.id,
          child: Text('${c.id} · ${c.kind}${c.enabled ? "" : " (disabled)"}',
              overflow: TextOverflow.ellipsis),
        ),
      if (base != null &&
          base.forgeId.isNotEmpty &&
          !cards.any((c) => c.id == base.forgeId))
        DropdownMenuItem(
            value: base.forgeId, child: Text('${base.forgeId} (not found)')),
    ];
  }

  @override
  Widget build(BuildContext context) {
    final isNew = widget.base == null;
    final items = _forgeItems();
    final value = items.any((i) => i.value == _forgeId) ? _forgeId : null;
    final card = widget.cards?.where((c) => c.id == _forgeId).firstOrNull;
    final pathRepo = card?.repoEncoding == 'path';
    return AlertDialog(
      title: Text(isNew ? 'New fleet session' : 'Edit ${widget.base!.id}'),
      content: SizedBox(
        width: 420,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              TextField(
                key: const Key('fleet.session.dialog.id'),
                controller: _id,
                enabled: isNew,
                autofocus: isNew,
                decoration: const InputDecoration(labelText: 'Session id'),
              ),
              const SizedBox(height: 8),
              DropdownButtonFormField<String>(
                key: const Key('fleet.session.dialog.forge'),
                initialValue: value,
                isExpanded: true,
                items: items,
                onChanged: (v) => setState(() => _forgeId = v ?? ''),
                decoration: InputDecoration(
                  labelText: 'Forge card',
                  helperText: widget.cards == null
                      ? 'Forge cards could not be listed'
                          '${widget.cardsError == null ? "" : ": ${widget.cardsError}"}'
                      : items.isEmpty
                          ? 'No forge cards yet — create one on the Forges tab'
                          : null,
                  helperMaxLines: 2,
                ),
              ),
              const SizedBox(height: 8),
              TextField(
                key: const Key('fleet.session.dialog.repo'),
                controller: _repo,
                decoration: InputDecoration(
                  labelText: 'Repository',
                  hintText: pathRepo ? 'group/subgroup/name' : 'owner/name',
                ),
              ),
              const SizedBox(height: 8),
              TextField(
                key: const Key('fleet.session.dialog.user'),
                controller: _user,
                enabled: isNew && (widget.tenant ?? '').isEmpty,
                decoration: InputDecoration(
                  labelText: 'Workspace owner',
                  helperText: !isNew
                      ? 'Fixed once created'
                      : (widget.tenant ?? '').isNotEmpty
                          ? 'Your tenant'
                          : 'Blank: the repository owner',
                ),
              ),
              const SizedBox(height: 8),
              TextField(
                key: const Key('fleet.session.dialog.skill'),
                controller: _skill,
                decoration: const InputDecoration(
                  labelText: 'Review skill',
                  helperText: 'Blank: the default review skill',
                ),
              ),
              const SizedBox(height: 8),
              TextField(
                key: const Key('fleet.session.dialog.poll'),
                controller: _poll,
                keyboardType: TextInputType.number,
                decoration: const InputDecoration(
                  labelText: 'Poll interval (seconds)',
                  helperText: 'Blank: the agent default',
                ),
              ),
              const SizedBox(height: 8),
              SwitchListTile(
                key: const Key('fleet.session.dialog.enabled'),
                contentPadding: EdgeInsets.zero,
                title: const Text('Enabled'),
                subtitle: const Text('The fleet polls and reviews only enabled sessions'),
                value: _enabled,
                onChanged: (v) => setState(() => _enabled = v),
              ),
              if (_error != null)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: Text(_error!,
                      key: const Key('fleet.session.dialog.error'),
                      style: TextStyle(color: Theme.of(context).colorScheme.error)),
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          key: const Key('fleet.session.dialog.cancel'),
          onPressed: () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
        FilledButton(
          key: const Key('fleet.session.dialog.save'),
          onPressed: _submit,
          child: Text(isNew ? 'Create' : 'Save'),
        ),
      ],
    );
  }
}
