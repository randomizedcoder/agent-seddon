import 'package:agent_portal/src/gen/agent/v1/forge_registry.pb.dart';
import 'package:agent_portal/src/gen/agent/v1/review_fleet.pb.dart';
import 'package:agent_portal/src/pages/fleet_session_dialog.dart';
import 'package:fixnum/fixnum.dart';
import 'package:flutter_test/flutter_test.dart';

/// L0 tables for the Fleet tab's session editor (forge-auth F5d2): the repo
/// slug encoding (mirrors the agent's `decode_owner_name` / `decode_path`) and
/// [buildSession] — what a new or edited row becomes, and every refusal.
void main() {
  group('repoSlug', () {
    for (final (name, input, ownerName, want) in [
      ('positive_owner_slash_name', 'acme/widgets', true, 'acme__widgets'),
      ('positive_already_encoded', 'my.org__a-b', true, 'my.org__a-b'),
      ('positive_trimmed', '  acme/w  ', true, 'acme__w'),
      ('positive_path_groups', 'grp/sub/proj', false, 'grp__sub__proj'),
      ('corner_path_single_segment', 'proj', false, 'proj'),
      ('corner_owner_name_triple_underscore', 'a___b', true, 'a___b'),
      ('negative_bare_owner', 'acme', true, null),
      ('negative_empty', '', true, null),
      ('negative_trailing_slash', 'acme/', true, null),
      ('negative_leading_slash', '/acme/x', true, null),
      ('negative_empty_name_encoded', 'acme__', true, null),
      ('negative_empty_owner_encoded', '__x', true, null),
      ('boundary_at_cap', '${'a' * 62}/${'b' * 64}', true, '${'a' * 62}__${'b' * 64}'),
      ('boundary_over_cap', '${'a' * 63}/${'b' * 64}', true, null),
      ('adversarial_traversal', '../../etc', true, null),
      ('adversarial_dotdot_segment', 'acme/../x', false, null),
      ('adversarial_leading_dash', 'acme/-rf', true, null),
      ('adversarial_backslash', r'acme\x', true, null),
      ('adversarial_query', 'acme/x?y', true, null),
      ('adversarial_space', 'a b/c', true, null),
      ('adversarial_fullwidth', 'ａcme/x', true, null),
      ('adversarial_url', 'https://github.com/acme/x', true, null),
    ]) {
      test(name, () => expect(repoSlug(input, ownerName: ownerName), want));
    }
  });

  group('validSessionSegment', () {
    for (final (name, s, ok) in [
      ('positive_plain', 's1', true),
      ('boundary_max', 'a' * maxSessionSegmentLen, true),
      ('boundary_over', 'a' * (maxSessionSegmentLen + 1), false),
      ('negative_empty', '', false),
      ('adversarial_dotdot', '..', false),
      ('adversarial_inner_dotdot', 'a..b', false),
      ('adversarial_leading_dot', '.git', false),
      ('adversarial_leading_dash', '-rf', false),
      ('adversarial_separator', 'a/b', false),
      ('adversarial_newline', 'a\nb', false),
      ('adversarial_non_ascii', 'ïd', false),
    ]) {
      test(name, () => expect(validSessionSegment(s), ok));
    }
  });

  group('buildSession', () {
    final gh = ForgeCard(id: 'github-42', kind: 'github', repoEncoding: 'owner_name', enabled: true);
    final gl = ForgeCard(id: 'gl-1', kind: 'gitlab', repoEncoding: 'path', enabled: true);
    final cards = [gh, gl];
    const ok = SessionDraft(id: 'w', forgeId: 'github-42', repo: 'acme/w');

    ({FleetSession? session, String? error}) build(SessionDraft d,
            {FleetSession? base, List<ForgeCard>? cs, Set<String> taken = const {}}) =>
        buildSession(d, base: base, cards: cs ?? cards, takenIds: taken);

    SessionDraft draftOf({
      String? id,
      String? forgeId,
      String? repo,
      String? user,
      String? skill,
      String? poll,
    }) =>
        SessionDraft(
          id: id ?? ok.id,
          forgeId: forgeId ?? ok.forgeId,
          repo: repo ?? ok.repo,
          user: user ?? '',
          skill: skill ?? '',
          poll: poll ?? '',
        );

    test('positive_new_row', () {
      final s = build(ok).session!;
      expect((s.id, s.forgeId, s.repo, s.user), ('w', 'github-42', 'acme__w', 'acme'));
      expect((s.backend, s.baseUrl, s.tokenRef, s.enabled), ('', '', '', false));
      expect(s.pollSecs, Int64.ZERO);
    });

    test('positive_path_card_user_is_group', () {
      final s = build(draftOf(forgeId: 'gl-1', repo: 'grp/sub/p')).session!;
      expect((s.repo, s.user), ('grp__sub__p', 'grp'));
    });

    test('positive_edit_keeps_unshown_and_id', () {
      final base = FleetSession(
          id: 's1',
          user: 'acme',
          forgeId: 'github-42',
          repo: 'acme__w',
          slackTriggerChannel: 'C1',
          transportId: 't1',
          createdAt: Int64(5));
      final s = build(draftOf(id: 'ignored', repo: 'acme/w'), base: base, taken: {'s1'}).session!;
      expect((s.id, s.slackTriggerChannel, s.transportId, s.createdAt), ('s1', 'C1', 't1', Int64(5)));
      expect(base.repo, 'acme__w', reason: 'the base row is not mutated');
    });

    test('corner_edit_inline_row_keeps_inline', () {
      final base = FleetSession(id: 's1', user: 'acme', backend: 'github', tokenRef: 'env:GH');
      final s = build(draftOf(forgeId: ''), base: base).session!;
      expect((s.forgeId, s.backend, s.tokenRef), ('', 'github', 'env:GH'));
    });

    test('corner_edit_card_clears_inline', () {
      final base = FleetSession(id: 's1', user: 'acme', backend: 'github', baseUrl: 'https://x', tokenRef: 'env:GH');
      final s = build(ok, base: base).session!;
      expect((s.forgeId, s.backend, s.baseUrl, s.tokenRef), ('github-42', '', '', ''));
    });

    test('corner_edit_keeps_missing_card', () {
      final base = FleetSession(id: 's1', user: 'acme', forgeId: 'gone');
      expect(build(draftOf(forgeId: 'gone'), base: base, cs: null).session!.forgeId, 'gone');
    });

    test('positive_poll_and_skill', () {
      final s = build(draftOf(poll: '600', skill: 'review-sec', user: 'bots')).session!;
      expect((s.pollSecs.toInt(), s.skill, s.user), (600, 'review-sec', 'bots'));
    });

    for (final (name, draft, base, cs, taken, why) in <(
      String,
      SessionDraft,
      FleetSession?,
      List<ForgeCard>?,
      Set<String>,
      String
    )>[
      ('negative_duplicate_id', ok, null, null, {'w'}, 'already exists'),
      ('negative_new_needs_card', draftOf(forgeId: ''), null, null, {}, 'Choose a forge card'),
      ('negative_cards_unlisted', ok, null, const <ForgeCard>[], {}, 'not found'),
      ('negative_card_unknown', draftOf(forgeId: 'nope'), null, null, {}, 'not found'),
      ('negative_edit_cannot_drop_card', draftOf(forgeId: ''), FleetSession(id: 's1', forgeId: 'github-42'), null, {}, 'Choose a forge card'),
      ('negative_owner_name_needs_owner', draftOf(repo: 'w'), null, null, {}, 'Repository'),
      ('boundary_poll_below_min', draftOf(poll: '${minPollSecs - 1}'), null, null, {}, 'Poll'),
      ('boundary_poll_above_max', draftOf(poll: '${maxPollSecs + 1}'), null, null, {}, 'Poll'),
      ('boundary_skill_over_cap', draftOf(skill: 'é' * 129), null, null, {}, 'Skill'),
      ('adversarial_id_traversal', draftOf(id: '../x'), null, null, {}, 'Session id'),
      ('adversarial_card_id_traversal', draftOf(forgeId: '../../x'), null, null, {}, 'not valid'),
      ('adversarial_user_traversal', draftOf(user: '../root'), null, null, {}, 'Workspace owner'),
      ('adversarial_skill_control', draftOf(skill: 'a\u0000b'), null, null, {}, 'Skill'),
      ('adversarial_poll_negative', draftOf(poll: '-30'), null, null, {}, 'Poll'),
      ('adversarial_poll_huge', draftOf(poll: '99999999999999999999'), null, null, {}, 'Poll'),
      ('adversarial_poll_float', draftOf(poll: '1e3'), null, null, {}, 'Poll'),
    ]) {
      test(name, () {
        final r = build(draft, base: base, cs: cs, taken: taken);
        expect(r.session, isNull);
        expect(r.error, contains(why));
      });
    }

    test('positive_signed_in_tenant_owns_new_row', () {
      final s = buildSession(draftOf(user: 'someone-else'),
              cards: cards, takenIds: const {}, tenant: 'example.com')
          .session!;
      expect(s.user, 'example.com');
    });

    test('adversarial_edit_keeps_owner', () {
      final base = FleetSession(id: 's1', user: 'tenant-a', forgeId: 'github-42');
      final s = buildSession(draftOf(user: 'tenant-b'),
              base: base, cards: cards, takenIds: const {}, tenant: 'tenant-b')
          .session!;
      expect(s.user, 'tenant-a', reason: 'an edit never moves a row to another tenant');
    });

    test('adversarial_hostile_tenant_refused', () {
      final r = buildSession(ok, cards: cards, takenIds: const {}, tenant: '../root');
      expect(r.session, isNull);
      expect(r.error, contains('Workspace owner'));
    });

    test('boundary_poll_edges_accepted', () {
      for (final p in ['0', '$minPollSecs', '$maxPollSecs']) {
        expect(build(draftOf(poll: p)).session, isNotNull, reason: p);
      }
    });
  });
}
