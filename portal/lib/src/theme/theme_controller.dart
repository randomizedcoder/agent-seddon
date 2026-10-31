// The active theme and light/dark mode, above `MaterialApp`
// (docs/design/visual/05-portal-integration.md). Once a user is known the
// server holds the choice (`ThemeService` `GetActive`/`SetActive`,
// docs/design/visual/04-per-user-active-theme.md); browser storage is only a
// cache, so the page paints the right theme before the server answers.
import 'dart:async';
import 'dart:convert';

import 'package:crypto/crypto.dart';
import 'package:flutter/material.dart';

import '../gen/agent/v1/theme.pb.dart' as pb;
import '../gen/agent/v1/theme.pbgrpc.dart' show ThemeServiceClient;
import '../io/graph_platform.dart' as storage;
import 'builtin_themes.dart';
import 'theme_mapper.dart';
import 'theme_validate.dart';

/// Where the controller keeps its cache: a key→string store.
class ThemePrefsStore {
  const ThemePrefsStore({String? Function(String)? read, void Function(String, String)? write})
      : _read = read ?? storage.loadRaw,
        _write = write ?? storage.saveRaw;

  final String? Function(String) _read;
  final void Function(String, String) _write;

  /// The browser's last theme id and mode, painted before anyone is signed in.
  /// Only a built-in id is ever restored from it, so it never shows one user's
  /// own theme to the next person at the sign-in page.
  static const themeKey = 'agent_portal.theme.v1';
  static const modeKey = 'agent_portal.theme_mode.v1';

  /// One user's last resolved theme document and mode, under [userKey].
  static const userPrefix = 'agent_portal.user_theme.v1.';

  /// The cache key for a user: a hash of tenant and subject, so storage holds
  /// no identifier and two tenants' `alice` never share a slot.
  static String userKey(String tenant, String subject) {
    final digest = sha256.convert(utf8.encode('$tenant\u0000$subject'));
    return '$userPrefix${digest.toString().substring(0, 32)}';
  }

  String? read(String key) => _read(key);
  void write(String key, String value) => _write(key, value);
}

/// The server half of the active theme. An interface, so the controller is
/// tested without a channel.
abstract class ThemeServer {
  Future<pb.ActiveTheme> getActive();
  Future<pb.ActiveTheme> setActive(String themeId, pb.ThemeMode mode);

  /// Built-ins, the tenant's shared themes and the caller's private ones.
  Future<List<pb.Theme>> list();

  /// Store a user theme; answers the stored copy (with its new `version`).
  Future<pb.Theme> put(pb.Theme theme);

  Future<void> delete(String id);

  /// Parse textproto on the server: the draft, and why `Put` would refuse it
  /// (empty when it would not). Stores nothing.
  Future<(pb.Theme, String)> importText(String text);

  /// [theme] as textproto. Stores nothing.
  Future<String> exportText(pb.Theme theme);
}

/// [ThemeServer] over the generated `ThemeService` client.
class GrpcThemeServer implements ThemeServer {
  GrpcThemeServer(this.client);
  final ThemeServiceClient client;

  @override
  Future<pb.ActiveTheme> getActive() => client.getActive(pb.GetActiveThemeRequest());

  @override
  Future<pb.ActiveTheme> setActive(String themeId, pb.ThemeMode mode) =>
      client.setActive(pb.SetActiveThemeRequest(themeId: themeId, mode: mode));

  @override
  Future<List<pb.Theme>> list() async =>
      (await client.list(pb.ListThemesRequest())).themes;

  @override
  Future<pb.Theme> put(pb.Theme theme) => client.put(theme);

  @override
  Future<void> delete(String id) => client.delete(pb.DeleteThemeRequest(id: id));

  @override
  Future<(pb.Theme, String)> importText(String text) async {
    final r = await client.import(pb.ImportThemeRequest(text: text));
    return (r.theme, r.problem);
  }

  @override
  Future<String> exportText(pb.Theme theme) async =>
      (await client.export(pb.ExportThemeRequest(theme: theme))).text;
}

ThemeMode _modeFromName(String? name) => switch (name) {
      'light' => ThemeMode.light,
      'dark' => ThemeMode.dark,
      _ => ThemeMode.system,
    };

/// The portal's mode as the wire enum, and back.
pb.ThemeMode toWireMode(ThemeMode m) => switch (m) {
      ThemeMode.light => pb.ThemeMode.THEME_MODE_LIGHT,
      ThemeMode.dark => pb.ThemeMode.THEME_MODE_DARK,
      ThemeMode.system => pb.ThemeMode.THEME_MODE_SYSTEM,
    };

ThemeMode fromWireMode(pb.ThemeMode m) => switch (m) {
      pb.ThemeMode.THEME_MODE_LIGHT => ThemeMode.light,
      pb.ThemeMode.THEME_MODE_DARK => ThemeMode.dark,
      _ => ThemeMode.system,
    };

class ThemeController extends ChangeNotifier {
  /// [themes] are the built-ins: validated, and containing [fallbackThemeId].
  ThemeController({
    required List<pb.Theme> themes,
    this.store = const ThemePrefsStore(),
    void Function(String)? log,
  })  : builtins = List.unmodifiable(themes),
        _log = log ?? debugPrint {
    _restoreDevice();
    _rebuild();
  }

  /// Load the bundled built-ins and the browser's last choice.
  static Future<ThemeController> load(AssetBundle bundle,
          {ThemePrefsStore store = const ThemePrefsStore()}) async =>
      ThemeController(themes: await loadBuiltinThemes(bundle), store: store);

  final List<pb.Theme> builtins;
  final ThemePrefsStore store;
  final void Function(String) _log;

  /// The signed-in user's own and shared themes, from `List` (validated).
  List<pb.Theme> _userThemes = const [];

  late pb.Theme _active;
  late ThemeMode _mode;
  late bool _hasSavedTheme;
  late ThemeData _light, _dark;
  ThemeData? _hcLight, _hcDark;

  ThemeServer? _server;
  String? _userKey;

  /// Bumped by every attach and detach, so a reply for an earlier user is
  /// dropped.
  int _identity = 0;

  /// Bumped by every attach, detach and pick, so a reply that arrives after
  /// a newer choice is dropped rather than undoing it.
  int _epoch = 0;

  /// Every theme the picker offers: the built-ins, then the user's.
  List<pb.Theme> get themes => List.unmodifiable([...builtins, ..._userThemes]);
  pb.Theme get active => _active;
  ThemeMode get mode => _mode;
  ThemeData get light => _light;
  ThemeData get dark => _dark;

  /// Whether a server holds this user's choice (signed in, or sign-in off).
  bool get attached => _server != null;

  /// The server the theme editor saves to; null until a user is known.
  ThemeServer? get server => _server;

  /// For the OS "increase contrast" setting: the `accessible` theme, but only
  /// while the user has not picked a theme themselves (08-accessibility.md).
  ThemeData? get highContrastLight => _hasSavedTheme ? null : _hcLight;
  ThemeData? get highContrastDark => _hasSavedTheme ? null : _hcDark;

  pb.Theme get _fallback => _builtin(fallbackThemeId)!;

  pb.Theme? _builtin(String id) => _find(builtins, id);

  pb.Theme? _byId(String id) => _builtin(id) ?? _find(_userThemes, id);

  static pb.Theme? _find(List<pb.Theme> list, String id) {
    for (final t in list) {
      if (t.id == id) return t;
    }
    return null;
  }

  /// Use [server] for the user [tenant]/[subject]: paint their cached theme at
  /// once, then ask the server. Calling it again for the same user (a token
  /// refresh) does nothing.
  Future<void> attach(ThemeServer server, {required String tenant, required String subject}) async {
    final key = ThemePrefsStore.userKey(tenant, subject);
    if (key == _userKey && identical(server, _server)) return;
    final identity = ++_identity;
    final epoch = ++_epoch;
    _server = server;
    _userKey = key;
    _userThemes = const [];
    _restoreUser(key);
    // A cached paint, not a choice: the browser-wide key keeps its value.
    _changed(device: false);

    // The list only fills the picker; without it the active theme still works.
    final listed = server.list().then<List<pb.Theme>?>((l) => l, onError: (Object e) {
      _log('theme: List failed: $e');
      return null;
    });
    final pb.ActiveTheme answer;
    try {
      answer = await server.getActive();
    } catch (e) {
      // An agent without ThemeService, or one that is down: keep the cache.
      _log('theme: GetActive failed, keeping the cached theme: $e');
      _list(identity, await listed);
      return;
    }
    _list(identity, await listed);
    // A pick made while GetActive was in flight wins over its answer.
    if (epoch == _epoch) _apply(answer);
  }

  /// Fill the picker with [listed], keeping the active theme in it.
  void _list(int identity, List<pb.Theme>? listed) {
    if (identity != _identity || listed == null) return;
    final users = _valid(listed);
    if (!_active.builtin && _find(users, _active.id) == null) users.add(_active);
    _userThemes = users;
    _changed(rebuild: false, device: false);
  }

  /// Forget the user (signed out): back to the browser's built-in choice.
  void detach() {
    if (_server == null) return;
    _identity++;
    _epoch++;
    _server = null;
    _userKey = null;
    _userThemes = const [];
    _restoreDevice();
    _changed(device: false);
  }

  /// Apply the theme [id], cache it and tell the server. An unknown id is
  /// ignored.
  void select(String id) {
    final t = _byId(id);
    if (t == null) return;
    _active = t;
    _hasSavedTheme = true;
    _changed();
    _push();
  }

  /// The editor stored [theme]: offer it, and make it the active theme. A
  /// theme that fails validation here (the server should never return one) is
  /// not offered, and the result is false.
  bool saved(pb.Theme theme) {
    final t = _checked(theme.deepCopy());
    if (t == null) return false;
    _userThemes = [
      for (final u in _userThemes)
        if (u.id != t.id) u,
      t,
    ];
    select(t.id);
    return true;
  }

  /// The editor deleted the user theme [id]: stop offering it, and fall back
  /// to the default built-in if it was active.
  void removed(String id) {
    if (_find(_userThemes, id) == null) return;
    _userThemes = [
      for (final u in _userThemes)
        if (u.id != id) u,
    ];
    if (_active.id == id) {
      select(fallbackThemeId);
    } else {
      _changed(rebuild: false, device: false);
    }
  }

  void setMode(ThemeMode mode) {
    _mode = mode;
    _changed(rebuild: false);
    _push();
  }

  /// A server answer: a valid theme is painted and cached; an invalid one (the
  /// server should never send it, but the document is user-authored) paints the
  /// fallback.
  void _apply(pb.ActiveTheme a) {
    if (!a.hasTheme()) {
      _log('theme: GetActive answered without a theme; keeping the cached one');
      return;
    }
    _mode = fromWireMode(a.mode);
    _hasSavedTheme = a.source == pb.ActiveSource.ACTIVE_SOURCE_USER;
    final t = a.theme;
    // A built-in is taken from the bundle, not the wire.
    final pb.Theme? chosen = t.builtin ? _builtin(t.id) : _checked(t);
    if (chosen == null) {
      _log('theme: the server sent an unusable theme `${t.id}`; using $fallbackThemeId');
    } else if (!chosen.builtin && _find(_userThemes, chosen.id) == null) {
      _userThemes = [..._userThemes, chosen];
    }
    _active = chosen ?? _fallback;
    _changed();
  }

  /// Send the current choice; a failure is logged and the local choice kept.
  void _push() {
    final server = _server;
    if (server == null) return;
    final epoch = ++_epoch;
    unawaited(server.setActive(_active.id, toWireMode(_mode)).then((_) {}, onError: (Object e) {
      if (epoch == _epoch) _log('theme: SetActive failed: $e');
    }));
  }

  /// Rebuild if the theme may have changed, write the cache, tell listeners.
  /// [device] also records a built-in choice as the browser's, painted at the
  /// next sign-in page.
  void _changed({bool rebuild = true, bool device = true}) {
    if (rebuild) _rebuild();
    _save(device: device);
    notifyListeners();
  }

  void _save({required bool device}) {
    if (device) {
      store.write(ThemePrefsStore.modeKey, _mode.name);
      if (_active.builtin) store.write(ThemePrefsStore.themeKey, _active.id);
    }
    final key = _userKey;
    if (key == null) return;
    final doc = _active.builtin ? {'id': _active.id} : _active.toProto3Json();
    store.write(key, jsonEncode({'mode': _mode.name, 'theme': doc}));
  }

  void _restoreDevice() {
    final saved = store.read(ThemePrefsStore.themeKey);
    final t = saved == null ? null : _builtin(saved);
    _hasSavedTheme = t != null;
    _active = t ?? _fallback;
    _mode = _modeFromName(store.read(ThemePrefsStore.modeKey));
  }

  /// The user's cached theme, else `prototype` in system mode. The cache is
  /// browser storage, so it is parsed and validated like any other input.
  void _restoreUser(String key) {
    _active = _fallback;
    _mode = ThemeMode.system;
    _hasSavedTheme = false;
    final raw = store.read(key);
    if (raw == null) return;
    if (raw.length > maxThemeBytes + 1024) {
      _log('theme: ignoring an oversized cached theme');
      return;
    }
    try {
      final v = jsonDecode(raw);
      if (v is! Map<String, dynamic>) return;
      final mode = v['mode'];
      _mode = _modeFromName(mode is String ? mode : null);
      final doc = v['theme'];
      if (doc is! Map<String, dynamic>) return;
      final id = doc['id'];
      final pb.Theme? t = id is String && _builtin(id) != null
          ? _builtin(id)
          : _checked(pb.Theme()..mergeFromProto3Json(doc));
      if (t == null) return;
      if (!t.builtin) _userThemes = [t];
      _active = t;
      _hasSavedTheme = true;
    } catch (e) {
      _log('theme: ignoring an unreadable cached theme: $e');
    }
  }

  /// [t] if it is a valid user theme, else null. A user theme that claims a
  /// built-in's id is refused, so it cannot stand in for the bundled copy.
  pb.Theme? _checked(pb.Theme t) {
    if (_builtin(t.id) != null) return null;
    try {
      validateTheme(t);
      return t..builtin = false;
    } on ThemeError catch (e) {
      _log('theme: dropping `${t.id}`: $e');
      return null;
    }
  }

  /// The listed user themes that validate, in the server's order, one per id.
  List<pb.Theme> _valid(List<pb.Theme> listed) {
    final out = <pb.Theme>[];
    for (final t in listed) {
      if (t.builtin) continue;
      final ok = _checked(t);
      if (ok != null && _find(out, ok.id) == null) out.add(ok);
    }
    return out;
  }

  void _rebuild() {
    ThemeData map(pb.Theme t, Brightness b) =>
        mapThemeOrFallback(t, b, fallback: _fallback, log: _log);
    _light = map(_active, Brightness.light);
    _dark = map(_active, Brightness.dark);
    final hc = _builtin('accessible');
    _hcLight = hc == null ? null : map(hc, Brightness.light);
    _hcDark = hc == null ? null : map(hc, Brightness.dark);
  }
}

/// Makes the [ThemeController] reachable from the picker in Settings and the
/// nav rail.
class ThemeScope extends InheritedNotifier<ThemeController> {
  const ThemeScope({super.key, required ThemeController controller, required super.child})
      : super(notifier: controller);

  static ThemeController? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<ThemeScope>()?.notifier;
}
