import 'dart:async';
import 'dart:convert';

import 'package:agent_portal/src/gen/agent/v1/theme.pb.dart' as pb;
import 'package:agent_portal/src/theme/theme_controller.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import '../../testkit/themes.dart';

/// A [ThemeServer] in memory: [active] answers GetActive (or [fail] throws),
/// [gate] holds GetActive until completed, and every SetActive is recorded.
class MemoryServer implements ThemeServer {
  MemoryServer({pb.ActiveTheme? active, this.listed = const [], this.fail, this.gate})
      : active = active ?? pb.ActiveTheme();

  pb.ActiveTheme active;
  List<pb.Theme> listed;
  Object? fail;
  Completer<void>? gate;
  Object? failSet;
  int gets = 0;
  final sets = <(String, pb.ThemeMode)>[];

  @override
  Future<pb.ActiveTheme> getActive() async {
    gets++;
    await gate?.future;
    if (fail != null) throw fail!;
    return active;
  }

  @override
  Future<pb.ActiveTheme> setActive(String id, pb.ThemeMode mode) async {
    sets.add((id, mode));
    if (failSet != null) throw failSet!;
    return pb.ActiveTheme(theme: pb.Theme(id: id), mode: mode);
  }

  @override
  Future<List<pb.Theme>> list() async => listed;
}

pb.ActiveTheme answer(pb.Theme t,
        {pb.ThemeMode mode = pb.ThemeMode.THEME_MODE_SYSTEM,
        pb.ActiveSource source = pb.ActiveSource.ACTIVE_SOURCE_USER}) =>
    pb.ActiveTheme(theme: t, mode: mode, source: source);

/// What a user's cache slot holds for [t] in [mode].
String cached(pb.Theme t, [String mode = 'system']) => jsonEncode({
      'mode': mode,
      'theme': t.builtin ? {'id': t.id} : t.toProto3Json(),
    });

final aliceKey = ThemePrefsStore.userKey('acme', 'alice');

/// One attach: what the user's cache holds, what the server does, and the
/// theme, mode and high-contrast offer the portal ends up with.
class AttachInput {
  const AttachInput({this.cache, required this.server});
  final String? cache;
  final MemoryServer Function() server;
}

typedef Painted = (String id, ThemeMode mode, bool highContrastOffered);

/// `ThemeController.attach` (visual/04): the cache paints first, the server
/// answer wins, and every stored or served document is untrusted.
void main() {
  (ThemeController, Map<String, String>, List<String>) make([Map<String, String>? saved]) {
    final mem = saved ?? <String, String>{};
    final logs = <String>[];
    final c = ThemeController(
      themes: builtinThemes(),
      store: ThemePrefsStore(read: (k) => mem[k], write: (k, v) => mem[k] = v),
      log: logs.add,
    );
    return (c, mem, logs);
  }

  final hostile = userTheme('evil')..light.onSurface = 'red; background:url(x)';
  final fakeBuiltin = userTheme('windows', name: 'Windows')..light.primary = '#FF0000';

  final attachCases = <ThemeCase<AttachInput, Painted>>[
    ThemeCase('positive_server_pick', "the user's stored pick and mode are painted",
        AttachInput(server: () => MemoryServer(
            active: answer(builtinTheme('windows'), mode: pb.ThemeMode.THEME_MODE_DARK))),
        ('windows', ThemeMode.dark, false)),
    ThemeCase('positive_user_theme', "the user's own theme is painted and joins the picker",
        AttachInput(server: () => MemoryServer(active: answer(userTheme('ocean')))),
        ('ocean', ThemeMode.system, false)),
    ThemeCase('corner_server_default', 'no pick: the server default is painted and high contrast is still offered',
        AttachInput(server: () => MemoryServer(active: answer(builtinTheme('macos'),
            source: pb.ActiveSource.ACTIVE_SOURCE_SERVER_DEFAULT))),
        ('macos', ThemeMode.system, true)),
    ThemeCase('negative_server_without_themes', 'UNIMPLEMENTED keeps the cached theme',
        AttachInput(cache: cached(builtinTheme('tn3270'), 'dark'),
            server: () => MemoryServer(fail: const GrpcError.unimplemented())),
        ('tn3270', ThemeMode.dark, false)),
    ThemeCase('negative_empty_cache_server_down', 'nothing cached and no server: prototype',
        AttachInput(server: () => MemoryServer(fail: const GrpcError.unavailable())),
        ('prototype', ThemeMode.system, true)),
    ThemeCase('positive_cached_user_theme', 'a cached user theme is painted while the server is down',
        AttachInput(cache: cached(userTheme('ocean')),
            server: () => MemoryServer(fail: const GrpcError.unavailable())),
        ('ocean', ThemeMode.system, false)),
    ThemeCase('adversarial_cache_spoofs_builtin', "a cached document with a built-in's id paints the bundled built-in",
        AttachInput(cache: jsonEncode({'mode': 'light', 'theme': fakeBuiltin.toProto3Json()}),
            server: () => MemoryServer(fail: const GrpcError.unavailable())),
        ('windows', ThemeMode.light, false)),
    ThemeCase('adversarial_cache_invalid_doc', 'a cached theme that fails validation is ignored',
        AttachInput(cache: cached(hostile),
            server: () => MemoryServer(fail: const GrpcError.unavailable())),
        ('prototype', ThemeMode.system, true)),
    ThemeCase('adversarial_cache_not_json', 'garbage in the cache is ignored',
        AttachInput(cache: '{"mode": <script>',
            server: () => MemoryServer(fail: const GrpcError.unavailable())),
        ('prototype', ThemeMode.system, true)),
    ThemeCase('adversarial_cache_wrong_shape', 'a cache that is JSON but not an object is ignored',
        AttachInput(cache: '[1,2,3]',
            server: () => MemoryServer(fail: const GrpcError.unavailable())),
        ('prototype', ThemeMode.system, true)),
    ThemeCase('adversarial_cache_huge', 'an oversized cache entry is not parsed',
        AttachInput(cache: '{"mode":"dark","theme":{"name":"${'x' * 200000}"}}',
            server: () => MemoryServer(fail: const GrpcError.unavailable())),
        ('prototype', ThemeMode.system, true)),
    ThemeCase('adversarial_server_invalid_theme', 'a served theme that fails validation paints the fallback',
        AttachInput(server: () => MemoryServer(active: answer(hostile))),
        ('prototype', ThemeMode.system, false)),
    ThemeCase('adversarial_server_user_theme_with_builtin_id', "a served user theme with a built-in's id is refused",
        AttachInput(server: () => MemoryServer(active: answer(fakeBuiltin))),
        ('prototype', ThemeMode.system, false)),
    ThemeCase('corner_answer_without_theme', 'an answer with no theme document keeps the cached theme and mode',
        AttachInput(cache: cached(builtinTheme('tn3270'), 'dark'),
            server: () => MemoryServer(active: pb.ActiveTheme())),
        ('tn3270', ThemeMode.dark, false)),
    ThemeCase('boundary_unspecified_mode', 'an unspecified mode on the wire is System',
        AttachInput(cache: cached(builtinTheme('windows'), 'dark'),
            server: () => MemoryServer(active: answer(builtinTheme('windows'),
                mode: pb.ThemeMode.THEME_MODE_UNSPECIFIED))),
        ('windows', ThemeMode.system, false)),
  ];
  for (final c in attachCases) {
    test('${c.name}: ${c.description}', () async {
      final (ctl, _, _) = make({if (c.input.cache != null) aliceKey: c.input.cache!});
      await ctl.attach(c.input.server(), tenant: 'acme', subject: 'alice');
      final (id, mode, hc) = c.expected;
      expect(ctl.active.id, id, reason: c.description);
      expect(ctl.mode, mode, reason: c.description);
      expect(ctl.highContrastLight != null, hc, reason: c.description);
      expect(ctl.themes.where((t) => t.id == 'evil'), isEmpty, reason: 'never offered');
      expect(ctl.light.colorScheme.primary, isNot(const Color(0xFFFF0000)),
          reason: 'a spoofed built-in is never painted');
    });
  }

  final modeCases = <ThemeCase<ThemeMode, pb.ThemeMode>>[
    const ThemeCase('positive_light', 'light maps both ways', ThemeMode.light, pb.ThemeMode.THEME_MODE_LIGHT),
    const ThemeCase('positive_dark', 'dark maps both ways', ThemeMode.dark, pb.ThemeMode.THEME_MODE_DARK),
    const ThemeCase('corner_system', 'system is sent explicitly, never as unspecified',
        ThemeMode.system, pb.ThemeMode.THEME_MODE_SYSTEM),
  ];
  for (final c in modeCases) {
    test('${c.name}: ${c.description}', () {
      expect(toWireMode(c.input), c.expected);
      expect(fromWireMode(c.expected), c.input);
    });
  }

  final keyCases = <ThemeCase<(String, String, String, String), bool>>[
    const ThemeCase('positive_same_user', 'the same user always gets the same slot',
        ('acme', 'alice', 'acme', 'alice'), true),
    const ThemeCase('negative_other_user', 'two users never share a slot',
        ('acme', 'alice', 'acme', 'bob'), false),
    const ThemeCase('corner_same_subject_other_tenant', "two tenants' alice never share a slot",
        ('acme', 'alice', 'globex', 'alice'), false),
    const ThemeCase('adversarial_separator_shift', 'moving characters across the tenant/subject boundary changes the slot',
        ('ac', 'mealice', 'acme', 'alice'), false),
    const ThemeCase('boundary_empty', 'empty tenant and subject still get a well-formed slot',
        ('', '', '', ''), true),
  ];
  for (final c in keyCases) {
    test('${c.name}: ${c.description}', () {
      final (t1, s1, t2, s2) = c.input;
      final a = ThemePrefsStore.userKey(t1, s1), b = ThemePrefsStore.userKey(t2, s2);
      expect(a == b, c.expected);
      expect(a, matches(RegExp(r'^agent_portal\.user_theme\.v1\.[0-9a-f]{32}$')));
      expect(a.contains(s1) && s1.isNotEmpty, isFalse, reason: 'no identifier in storage');
    });
  }

  test('positive_pick_sent_and_cached: a pick goes to the server and the user cache', () async {
    final (ctl, mem, _) = make();
    final server = MemoryServer(listed: [userTheme('ocean')]);
    await ctl.attach(server, tenant: 'acme', subject: 'alice');
    ctl.select('ocean');
    ctl.setMode(ThemeMode.dark);
    await pumpEventQueue();
    expect(server.sets, [
      ('ocean', pb.ThemeMode.THEME_MODE_SYSTEM),
      ('ocean', pb.ThemeMode.THEME_MODE_DARK),
    ]);
    final slot = jsonDecode(mem[aliceKey]!) as Map<String, dynamic>;
    expect(slot['mode'], 'dark');
    expect((slot['theme'] as Map)['id'], 'ocean');
    expect(mem[ThemePrefsStore.themeKey], isNot('ocean'),
        reason: "the browser-wide key never holds a user's own theme");
  });

  test('corner_pick_during_get_active_wins: an answer that arrives after a pick is dropped', () async {
    final (ctl, _, _) = make();
    final server = MemoryServer(
        active: answer(builtinTheme('tn3270')), gate: Completer<void>());
    final attaching = ctl.attach(server, tenant: 'acme', subject: 'alice');
    ctl.select('macos');
    server.gate!.complete();
    await attaching;
    expect(ctl.active.id, 'macos');
    expect(server.sets.single.$1, 'macos');
  });

  test('corner_late_answer_for_previous_user: a reply for the last user is not painted for the next', () async {
    final (ctl, _, _) = make();
    final alice = MemoryServer(
        active: answer(builtinTheme('tn3270')),
        listed: [userTheme('alice-only')],
        gate: Completer<void>());
    final bob = MemoryServer(active: answer(builtinTheme('windows')));
    final a = ctl.attach(alice, tenant: 'acme', subject: 'alice');
    await ctl.attach(bob, tenant: 'acme', subject: 'bob');
    alice.gate!.complete();
    await a;
    expect(ctl.active.id, 'windows');
    expect(ctl.themes.map((t) => t.id), isNot(contains('alice-only')));
  });

  test('corner_refresh_is_a_no_op: attaching the same user again asks nothing', () async {
    final (ctl, _, _) = make();
    final server = MemoryServer(active: answer(builtinTheme('windows')));
    await ctl.attach(server, tenant: 'acme', subject: 'alice');
    await ctl.attach(server, tenant: 'acme', subject: 'alice');
    expect(server.gets, 1);
  });

  test("positive_switch_user: the next user does not see the last user's theme", () async {
    final (ctl, mem, _) = make();
    await ctl.attach(MemoryServer(active: answer(userTheme('ocean'))),
        tenant: 'acme', subject: 'alice');
    expect(ctl.active.id, 'ocean');
    final bob = MemoryServer(fail: const GrpcError.unavailable());
    await ctl.attach(bob, tenant: 'acme', subject: 'bob');
    expect(ctl.active.id, 'prototype');
    expect(ctl.themes.map((t) => t.id), isNot(contains('ocean')));
    expect(mem[aliceKey], contains('ocean'), reason: "alice's slot is kept for her");
  });

  test('positive_detach_restores_the_browser_choice: signing out drops user themes', () async {
    final (ctl, _, _) = make({ThemePrefsStore.themeKey: 'macos'});
    await ctl.attach(MemoryServer(active: answer(userTheme('ocean'))),
        tenant: 'acme', subject: 'alice');
    ctl.detach();
    expect(ctl.attached, isFalse);
    expect(ctl.active.id, 'macos');
    expect(ctl.themes.map((t) => t.id), isNot(contains('ocean')));
  });

  test('negative_set_active_refused: the local choice stays and the failure is logged', () async {
    final (ctl, _, logs) = make();
    final server = MemoryServer(active: answer(builtinTheme('windows')))
      ..failSet = const GrpcError.permissionDenied();
    await ctl.attach(server, tenant: 'acme', subject: 'alice');
    ctl.select('tn3270');
    await pumpEventQueue();
    expect(ctl.active.id, 'tn3270');
    expect(logs.any((l) => l.contains('SetActive failed')), isTrue);
  });

  test('negative_list_fails: the active theme still resolves', () async {
    final (ctl, _, logs) = make();
    final server = _ListFails(active: answer(builtinTheme('windows')));
    await ctl.attach(server, tenant: 'acme', subject: 'alice');
    expect(ctl.active.id, 'windows');
    expect(logs.any((l) => l.contains('List failed')), isTrue);
  });

  test('boundary_unattached_pick_stays_local: signed out, a pick calls no server', () async {
    final (ctl, mem, _) = make();
    ctl.select('windows');
    expect(ctl.attached, isFalse);
    expect(mem[ThemePrefsStore.themeKey], 'windows');
    expect(mem.keys.where((k) => k.startsWith(ThemePrefsStore.userPrefix)), isEmpty);
  });
}

class _ListFails extends MemoryServer {
  _ListFails({super.active});
  @override
  Future<List<pb.Theme>> list() async => throw const GrpcError.unavailable();
}
