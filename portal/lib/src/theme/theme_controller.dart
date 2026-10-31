// The active theme and light/dark mode, above `MaterialApp`
// (docs/design/visual/05-portal-integration.md). Increment 2 keeps the choice
// in browser storage; increment 3 moves it server-side (`ThemeService`
// `GetActive`/`SetActive`) and keeps this cache only to avoid a flash on load.
import 'package:flutter/material.dart';

import '../gen/agent/v1/theme.pb.dart' as pb;
import '../io/graph_platform.dart' as storage;
import 'builtin_themes.dart';
import 'theme_mapper.dart';

/// Where the controller keeps the user's choice: a key→string store.
class ThemePrefsStore {
  const ThemePrefsStore({String? Function(String)? read, void Function(String, String)? write})
      : _read = read ?? storage.loadRaw,
        _write = write ?? storage.saveRaw;

  final String? Function(String) _read;
  final void Function(String, String) _write;

  static const themeKey = 'agent_portal.theme.v1';
  static const modeKey = 'agent_portal.theme_mode.v1';

  String? read(String key) => _read(key);
  void write(String key, String value) => _write(key, value);
}

class ThemeController extends ChangeNotifier {
  /// [themes] must be validated and contain [fallbackThemeId].
  ThemeController({
    required List<pb.Theme> themes,
    this.store = const ThemePrefsStore(),
    void Function(String)? log,
  })  : themes = List.unmodifiable(themes),
        _log = log ?? debugPrint {
    final saved = store.read(ThemePrefsStore.themeKey);
    _hasSavedTheme = saved != null && _byId(saved) != null;
    _active = (_hasSavedTheme ? _byId(saved!) : null) ?? _fallback;
    _mode = switch (store.read(ThemePrefsStore.modeKey)) {
      'light' => ThemeMode.light,
      'dark' => ThemeMode.dark,
      _ => ThemeMode.system,
    };
    _rebuild();
  }

  /// Load the bundled built-ins and the saved choice.
  static Future<ThemeController> load(AssetBundle bundle,
          {ThemePrefsStore store = const ThemePrefsStore()}) async =>
      ThemeController(themes: await loadBuiltinThemes(bundle), store: store);

  final List<pb.Theme> themes;
  final ThemePrefsStore store;
  final void Function(String) _log;

  late pb.Theme _active;
  late ThemeMode _mode;
  late bool _hasSavedTheme;
  late ThemeData _light, _dark;
  ThemeData? _hcLight, _hcDark;

  pb.Theme get active => _active;
  ThemeMode get mode => _mode;
  ThemeData get light => _light;
  ThemeData get dark => _dark;

  /// For the OS "increase contrast" setting: the `accessible` theme, but only
  /// while the user has not picked a theme themselves (08-accessibility.md).
  ThemeData? get highContrastLight => _hasSavedTheme ? null : _hcLight;
  ThemeData? get highContrastDark => _hasSavedTheme ? null : _hcDark;

  pb.Theme get _fallback => _byId(fallbackThemeId)!;

  pb.Theme? _byId(String id) {
    for (final t in themes) {
      if (t.id == id) return t;
    }
    return null;
  }

  /// Apply the theme [id] and remember it. An unknown id is ignored.
  void select(String id) {
    final t = _byId(id);
    if (t == null) return;
    _active = t;
    _hasSavedTheme = true;
    store.write(ThemePrefsStore.themeKey, id);
    _rebuild();
    notifyListeners();
  }

  void setMode(ThemeMode mode) {
    _mode = mode;
    store.write(ThemePrefsStore.modeKey, mode.name);
    notifyListeners();
  }

  void _rebuild() {
    ThemeData map(pb.Theme t, Brightness b) =>
        mapThemeOrFallback(t, b, fallback: _fallback, log: _log);
    _light = map(_active, Brightness.light);
    _dark = map(_active, Brightness.dark);
    final hc = _byId('accessible');
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
