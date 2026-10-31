// This is a generated file - do not edit.
//
// Generated from agent/v1/theme.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

import 'theme.pbenum.dart';

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

export 'theme.pbenum.dart';

/// One portal look. Built-ins ship as `config/themes/<id>.textproto`.
class Theme extends $pb.GeneratedMessage {
  factory Theme({
    $core.String? id,
    $core.String? name,
    $core.String? description,
    $core.String? author,
    $core.bool? builtin,
    $fixnum.Int64? version,
    Palette? light,
    Palette? dark,
    Typography? typography,
    Shape? shape,
    Density? density,
    Accessibility? accessibility,
    Visibility? visibility,
    $core.String? owner,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (name != null) result.name = name;
    if (description != null) result.description = description;
    if (author != null) result.author = author;
    if (builtin != null) result.builtin = builtin;
    if (version != null) result.version = version;
    if (light != null) result.light = light;
    if (dark != null) result.dark = dark;
    if (typography != null) result.typography = typography;
    if (shape != null) result.shape = shape;
    if (density != null) result.density = density;
    if (accessibility != null) result.accessibility = accessibility;
    if (visibility != null) result.visibility = visibility;
    if (owner != null) result.owner = owner;
    return result;
  }

  Theme._();

  factory Theme.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory Theme.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Theme',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..aOS(2, _omitFieldNames ? '' : 'name')
    ..aOS(3, _omitFieldNames ? '' : 'description')
    ..aOS(4, _omitFieldNames ? '' : 'author')
    ..aOB(5, _omitFieldNames ? '' : 'builtin')
    ..a<$fixnum.Int64>(6, _omitFieldNames ? '' : 'version', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOM<Palette>(7, _omitFieldNames ? '' : 'light',
        subBuilder: Palette.create)
    ..aOM<Palette>(8, _omitFieldNames ? '' : 'dark', subBuilder: Palette.create)
    ..aOM<Typography>(9, _omitFieldNames ? '' : 'typography',
        subBuilder: Typography.create)
    ..aOM<Shape>(10, _omitFieldNames ? '' : 'shape', subBuilder: Shape.create)
    ..aE<Density>(11, _omitFieldNames ? '' : 'density',
        enumValues: Density.values)
    ..aOM<Accessibility>(12, _omitFieldNames ? '' : 'accessibility',
        subBuilder: Accessibility.create)
    ..aE<Visibility>(13, _omitFieldNames ? '' : 'visibility',
        enumValues: Visibility.values)
    ..aOS(14, _omitFieldNames ? '' : 'owner')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Theme clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Theme copyWith(void Function(Theme) updates) =>
      super.copyWith((message) => updates(message as Theme)) as Theme;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static Theme create() => Theme._();
  @$core.override
  Theme createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static Theme getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<Theme>(create);
  static Theme? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get name => $_getSZ(1);
  @$pb.TagNumber(2)
  set name($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasName() => $_has(1);
  @$pb.TagNumber(2)
  void clearName() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get description => $_getSZ(2);
  @$pb.TagNumber(3)
  set description($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasDescription() => $_has(2);
  @$pb.TagNumber(3)
  void clearDescription() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get author => $_getSZ(3);
  @$pb.TagNumber(4)
  set author($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasAuthor() => $_has(3);
  @$pb.TagNumber(4)
  void clearAuthor() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.bool get builtin => $_getBF(4);
  @$pb.TagNumber(5)
  set builtin($core.bool value) => $_setBool(4, value);
  @$pb.TagNumber(5)
  $core.bool hasBuiltin() => $_has(4);
  @$pb.TagNumber(5)
  void clearBuiltin() => $_clearField(5);

  @$pb.TagNumber(6)
  $fixnum.Int64 get version => $_getI64(5);
  @$pb.TagNumber(6)
  set version($fixnum.Int64 value) => $_setInt64(5, value);
  @$pb.TagNumber(6)
  $core.bool hasVersion() => $_has(5);
  @$pb.TagNumber(6)
  void clearVersion() => $_clearField(6);

  @$pb.TagNumber(7)
  Palette get light => $_getN(6);
  @$pb.TagNumber(7)
  set light(Palette value) => $_setField(7, value);
  @$pb.TagNumber(7)
  $core.bool hasLight() => $_has(6);
  @$pb.TagNumber(7)
  void clearLight() => $_clearField(7);
  @$pb.TagNumber(7)
  Palette ensureLight() => $_ensure(6);

  @$pb.TagNumber(8)
  Palette get dark => $_getN(7);
  @$pb.TagNumber(8)
  set dark(Palette value) => $_setField(8, value);
  @$pb.TagNumber(8)
  $core.bool hasDark() => $_has(7);
  @$pb.TagNumber(8)
  void clearDark() => $_clearField(8);
  @$pb.TagNumber(8)
  Palette ensureDark() => $_ensure(7);

  @$pb.TagNumber(9)
  Typography get typography => $_getN(8);
  @$pb.TagNumber(9)
  set typography(Typography value) => $_setField(9, value);
  @$pb.TagNumber(9)
  $core.bool hasTypography() => $_has(8);
  @$pb.TagNumber(9)
  void clearTypography() => $_clearField(9);
  @$pb.TagNumber(9)
  Typography ensureTypography() => $_ensure(8);

  @$pb.TagNumber(10)
  Shape get shape => $_getN(9);
  @$pb.TagNumber(10)
  set shape(Shape value) => $_setField(10, value);
  @$pb.TagNumber(10)
  $core.bool hasShape() => $_has(9);
  @$pb.TagNumber(10)
  void clearShape() => $_clearField(10);
  @$pb.TagNumber(10)
  Shape ensureShape() => $_ensure(9);

  @$pb.TagNumber(11)
  Density get density => $_getN(10);
  @$pb.TagNumber(11)
  set density(Density value) => $_setField(11, value);
  @$pb.TagNumber(11)
  $core.bool hasDensity() => $_has(10);
  @$pb.TagNumber(11)
  void clearDensity() => $_clearField(11);

  @$pb.TagNumber(12)
  Accessibility get accessibility => $_getN(11);
  @$pb.TagNumber(12)
  set accessibility(Accessibility value) => $_setField(12, value);
  @$pb.TagNumber(12)
  $core.bool hasAccessibility() => $_has(11);
  @$pb.TagNumber(12)
  void clearAccessibility() => $_clearField(12);
  @$pb.TagNumber(12)
  Accessibility ensureAccessibility() => $_ensure(11);

  @$pb.TagNumber(13)
  Visibility get visibility => $_getN(12);
  @$pb.TagNumber(13)
  set visibility(Visibility value) => $_setField(13, value);
  @$pb.TagNumber(13)
  $core.bool hasVisibility() => $_has(12);
  @$pb.TagNumber(13)
  void clearVisibility() => $_clearField(13);

  @$pb.TagNumber(14)
  $core.String get owner => $_getSZ(13);
  @$pb.TagNumber(14)
  set owner($core.String value) => $_setString(13, value);
  @$pb.TagNumber(14)
  $core.bool hasOwner() => $_has(13);
  @$pb.TagNumber(14)
  void clearOwner() => $_clearField(14);
}

/// Colours are "#RRGGBB" or "#RRGGBBAA". Unset roles derive from `seed` (Material 3
/// tonal palette); with no seed every role is required.
class Palette extends $pb.GeneratedMessage {
  factory Palette({
    $core.String? seed,
    $core.String? primary,
    $core.String? onPrimary,
    $core.String? secondary,
    $core.String? onSecondary,
    $core.String? surface,
    $core.String? onSurface,
    $core.String? background,
    $core.String? surfaceVariant,
    $core.String? onSurfaceVariant,
    $core.String? outline,
    $core.String? error,
    $core.String? onError,
    $core.String? focus,
    SemanticColors? semantic,
  }) {
    final result = create();
    if (seed != null) result.seed = seed;
    if (primary != null) result.primary = primary;
    if (onPrimary != null) result.onPrimary = onPrimary;
    if (secondary != null) result.secondary = secondary;
    if (onSecondary != null) result.onSecondary = onSecondary;
    if (surface != null) result.surface = surface;
    if (onSurface != null) result.onSurface = onSurface;
    if (background != null) result.background = background;
    if (surfaceVariant != null) result.surfaceVariant = surfaceVariant;
    if (onSurfaceVariant != null) result.onSurfaceVariant = onSurfaceVariant;
    if (outline != null) result.outline = outline;
    if (error != null) result.error = error;
    if (onError != null) result.onError = onError;
    if (focus != null) result.focus = focus;
    if (semantic != null) result.semantic = semantic;
    return result;
  }

  Palette._();

  factory Palette.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory Palette.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Palette',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'seed')
    ..aOS(2, _omitFieldNames ? '' : 'primary')
    ..aOS(3, _omitFieldNames ? '' : 'onPrimary')
    ..aOS(4, _omitFieldNames ? '' : 'secondary')
    ..aOS(5, _omitFieldNames ? '' : 'onSecondary')
    ..aOS(6, _omitFieldNames ? '' : 'surface')
    ..aOS(7, _omitFieldNames ? '' : 'onSurface')
    ..aOS(8, _omitFieldNames ? '' : 'background')
    ..aOS(9, _omitFieldNames ? '' : 'surfaceVariant')
    ..aOS(10, _omitFieldNames ? '' : 'onSurfaceVariant')
    ..aOS(11, _omitFieldNames ? '' : 'outline')
    ..aOS(12, _omitFieldNames ? '' : 'error')
    ..aOS(13, _omitFieldNames ? '' : 'onError')
    ..aOS(14, _omitFieldNames ? '' : 'focus')
    ..aOM<SemanticColors>(15, _omitFieldNames ? '' : 'semantic',
        subBuilder: SemanticColors.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Palette clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Palette copyWith(void Function(Palette) updates) =>
      super.copyWith((message) => updates(message as Palette)) as Palette;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static Palette create() => Palette._();
  @$core.override
  Palette createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static Palette getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<Palette>(create);
  static Palette? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get seed => $_getSZ(0);
  @$pb.TagNumber(1)
  set seed($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasSeed() => $_has(0);
  @$pb.TagNumber(1)
  void clearSeed() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get primary => $_getSZ(1);
  @$pb.TagNumber(2)
  set primary($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasPrimary() => $_has(1);
  @$pb.TagNumber(2)
  void clearPrimary() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get onPrimary => $_getSZ(2);
  @$pb.TagNumber(3)
  set onPrimary($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasOnPrimary() => $_has(2);
  @$pb.TagNumber(3)
  void clearOnPrimary() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get secondary => $_getSZ(3);
  @$pb.TagNumber(4)
  set secondary($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasSecondary() => $_has(3);
  @$pb.TagNumber(4)
  void clearSecondary() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get onSecondary => $_getSZ(4);
  @$pb.TagNumber(5)
  set onSecondary($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasOnSecondary() => $_has(4);
  @$pb.TagNumber(5)
  void clearOnSecondary() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.String get surface => $_getSZ(5);
  @$pb.TagNumber(6)
  set surface($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasSurface() => $_has(5);
  @$pb.TagNumber(6)
  void clearSurface() => $_clearField(6);

  @$pb.TagNumber(7)
  $core.String get onSurface => $_getSZ(6);
  @$pb.TagNumber(7)
  set onSurface($core.String value) => $_setString(6, value);
  @$pb.TagNumber(7)
  $core.bool hasOnSurface() => $_has(6);
  @$pb.TagNumber(7)
  void clearOnSurface() => $_clearField(7);

  @$pb.TagNumber(8)
  $core.String get background => $_getSZ(7);
  @$pb.TagNumber(8)
  set background($core.String value) => $_setString(7, value);
  @$pb.TagNumber(8)
  $core.bool hasBackground() => $_has(7);
  @$pb.TagNumber(8)
  void clearBackground() => $_clearField(8);

  @$pb.TagNumber(9)
  $core.String get surfaceVariant => $_getSZ(8);
  @$pb.TagNumber(9)
  set surfaceVariant($core.String value) => $_setString(8, value);
  @$pb.TagNumber(9)
  $core.bool hasSurfaceVariant() => $_has(8);
  @$pb.TagNumber(9)
  void clearSurfaceVariant() => $_clearField(9);

  @$pb.TagNumber(10)
  $core.String get onSurfaceVariant => $_getSZ(9);
  @$pb.TagNumber(10)
  set onSurfaceVariant($core.String value) => $_setString(9, value);
  @$pb.TagNumber(10)
  $core.bool hasOnSurfaceVariant() => $_has(9);
  @$pb.TagNumber(10)
  void clearOnSurfaceVariant() => $_clearField(10);

  @$pb.TagNumber(11)
  $core.String get outline => $_getSZ(10);
  @$pb.TagNumber(11)
  set outline($core.String value) => $_setString(10, value);
  @$pb.TagNumber(11)
  $core.bool hasOutline() => $_has(10);
  @$pb.TagNumber(11)
  void clearOutline() => $_clearField(11);

  @$pb.TagNumber(12)
  $core.String get error => $_getSZ(11);
  @$pb.TagNumber(12)
  set error($core.String value) => $_setString(11, value);
  @$pb.TagNumber(12)
  $core.bool hasError() => $_has(11);
  @$pb.TagNumber(12)
  void clearError() => $_clearField(12);

  @$pb.TagNumber(13)
  $core.String get onError => $_getSZ(12);
  @$pb.TagNumber(13)
  set onError($core.String value) => $_setString(12, value);
  @$pb.TagNumber(13)
  $core.bool hasOnError() => $_has(12);
  @$pb.TagNumber(13)
  void clearOnError() => $_clearField(13);

  @$pb.TagNumber(14)
  $core.String get focus => $_getSZ(13);
  @$pb.TagNumber(14)
  set focus($core.String value) => $_setString(13, value);
  @$pb.TagNumber(14)
  $core.bool hasFocus() => $_has(13);
  @$pb.TagNumber(14)
  void clearFocus() => $_clearField(14);

  @$pb.TagNumber(15)
  SemanticColors get semantic => $_getN(14);
  @$pb.TagNumber(15)
  set semantic(SemanticColors value) => $_setField(15, value);
  @$pb.TagNumber(15)
  $core.bool hasSemantic() => $_has(14);
  @$pb.TagNumber(15)
  void clearSemantic() => $_clearField(15);
  @$pb.TagNumber(15)
  SemanticColors ensureSemantic() => $_ensure(14);
}

/// The colours the portal used to hardcode, named by meaning.
class SemanticColors extends $pb.GeneratedMessage {
  factory SemanticColors({
    $core.String? success,
    $core.String? warning,
    $core.String? info,
    $core.String? muted,
    $core.String? codeBackground,
    $core.String? codeForeground,
    $core.Iterable<$core.MapEntry<$core.String, $core.String>>? navTint,
    $core.Iterable<$core.MapEntry<$core.String, $core.String>>? graphEdge,
  }) {
    final result = create();
    if (success != null) result.success = success;
    if (warning != null) result.warning = warning;
    if (info != null) result.info = info;
    if (muted != null) result.muted = muted;
    if (codeBackground != null) result.codeBackground = codeBackground;
    if (codeForeground != null) result.codeForeground = codeForeground;
    if (navTint != null) result.navTint.addEntries(navTint);
    if (graphEdge != null) result.graphEdge.addEntries(graphEdge);
    return result;
  }

  SemanticColors._();

  factory SemanticColors.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SemanticColors.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SemanticColors',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'success')
    ..aOS(2, _omitFieldNames ? '' : 'warning')
    ..aOS(3, _omitFieldNames ? '' : 'info')
    ..aOS(4, _omitFieldNames ? '' : 'muted')
    ..aOS(5, _omitFieldNames ? '' : 'codeBackground')
    ..aOS(6, _omitFieldNames ? '' : 'codeForeground')
    ..m<$core.String, $core.String>(7, _omitFieldNames ? '' : 'navTint',
        entryClassName: 'SemanticColors.NavTintEntry',
        keyFieldType: $pb.PbFieldType.OS,
        valueFieldType: $pb.PbFieldType.OS,
        packageName: const $pb.PackageName('agent.v1'))
    ..m<$core.String, $core.String>(8, _omitFieldNames ? '' : 'graphEdge',
        entryClassName: 'SemanticColors.GraphEdgeEntry',
        keyFieldType: $pb.PbFieldType.OS,
        valueFieldType: $pb.PbFieldType.OS,
        packageName: const $pb.PackageName('agent.v1'))
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SemanticColors clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SemanticColors copyWith(void Function(SemanticColors) updates) =>
      super.copyWith((message) => updates(message as SemanticColors))
          as SemanticColors;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SemanticColors create() => SemanticColors._();
  @$core.override
  SemanticColors createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SemanticColors getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SemanticColors>(create);
  static SemanticColors? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get success => $_getSZ(0);
  @$pb.TagNumber(1)
  set success($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasSuccess() => $_has(0);
  @$pb.TagNumber(1)
  void clearSuccess() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get warning => $_getSZ(1);
  @$pb.TagNumber(2)
  set warning($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasWarning() => $_has(1);
  @$pb.TagNumber(2)
  void clearWarning() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get info => $_getSZ(2);
  @$pb.TagNumber(3)
  set info($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasInfo() => $_has(2);
  @$pb.TagNumber(3)
  void clearInfo() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get muted => $_getSZ(3);
  @$pb.TagNumber(4)
  set muted($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasMuted() => $_has(3);
  @$pb.TagNumber(4)
  void clearMuted() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get codeBackground => $_getSZ(4);
  @$pb.TagNumber(5)
  set codeBackground($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasCodeBackground() => $_has(4);
  @$pb.TagNumber(5)
  void clearCodeBackground() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.String get codeForeground => $_getSZ(5);
  @$pb.TagNumber(6)
  set codeForeground($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasCodeForeground() => $_has(5);
  @$pb.TagNumber(6)
  void clearCodeForeground() => $_clearField(6);

  @$pb.TagNumber(7)
  $pb.PbMap<$core.String, $core.String> get navTint => $_getMap(6);

  @$pb.TagNumber(8)
  $pb.PbMap<$core.String, $core.String> get graphEdge => $_getMap(7);
}

/// Font families must be bundled with the portal (no remote fonts).
class Typography extends $pb.GeneratedMessage {
  factory Typography({
    $core.String? bodyFamily,
    $core.String? headingFamily,
    $core.String? monoFamily,
    $core.double? scale,
    $core.int? bodyWeight,
    $core.int? headingWeight,
    $core.double? letterSpacing,
  }) {
    final result = create();
    if (bodyFamily != null) result.bodyFamily = bodyFamily;
    if (headingFamily != null) result.headingFamily = headingFamily;
    if (monoFamily != null) result.monoFamily = monoFamily;
    if (scale != null) result.scale = scale;
    if (bodyWeight != null) result.bodyWeight = bodyWeight;
    if (headingWeight != null) result.headingWeight = headingWeight;
    if (letterSpacing != null) result.letterSpacing = letterSpacing;
    return result;
  }

  Typography._();

  factory Typography.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory Typography.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Typography',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'bodyFamily')
    ..aOS(2, _omitFieldNames ? '' : 'headingFamily')
    ..aOS(3, _omitFieldNames ? '' : 'monoFamily')
    ..aD(4, _omitFieldNames ? '' : 'scale')
    ..aI(5, _omitFieldNames ? '' : 'bodyWeight', fieldType: $pb.PbFieldType.OU3)
    ..aI(6, _omitFieldNames ? '' : 'headingWeight',
        fieldType: $pb.PbFieldType.OU3)
    ..aD(7, _omitFieldNames ? '' : 'letterSpacing')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Typography clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Typography copyWith(void Function(Typography) updates) =>
      super.copyWith((message) => updates(message as Typography)) as Typography;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static Typography create() => Typography._();
  @$core.override
  Typography createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static Typography getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<Typography>(create);
  static Typography? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get bodyFamily => $_getSZ(0);
  @$pb.TagNumber(1)
  set bodyFamily($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasBodyFamily() => $_has(0);
  @$pb.TagNumber(1)
  void clearBodyFamily() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get headingFamily => $_getSZ(1);
  @$pb.TagNumber(2)
  set headingFamily($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasHeadingFamily() => $_has(1);
  @$pb.TagNumber(2)
  void clearHeadingFamily() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get monoFamily => $_getSZ(2);
  @$pb.TagNumber(3)
  set monoFamily($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasMonoFamily() => $_has(2);
  @$pb.TagNumber(3)
  void clearMonoFamily() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.double get scale => $_getN(3);
  @$pb.TagNumber(4)
  set scale($core.double value) => $_setDouble(3, value);
  @$pb.TagNumber(4)
  $core.bool hasScale() => $_has(3);
  @$pb.TagNumber(4)
  void clearScale() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.int get bodyWeight => $_getIZ(4);
  @$pb.TagNumber(5)
  set bodyWeight($core.int value) => $_setUnsignedInt32(4, value);
  @$pb.TagNumber(5)
  $core.bool hasBodyWeight() => $_has(4);
  @$pb.TagNumber(5)
  void clearBodyWeight() => $_clearField(5);

  @$pb.TagNumber(6)
  $core.int get headingWeight => $_getIZ(5);
  @$pb.TagNumber(6)
  set headingWeight($core.int value) => $_setUnsignedInt32(5, value);
  @$pb.TagNumber(6)
  $core.bool hasHeadingWeight() => $_has(5);
  @$pb.TagNumber(6)
  void clearHeadingWeight() => $_clearField(6);

  @$pb.TagNumber(7)
  $core.double get letterSpacing => $_getN(6);
  @$pb.TagNumber(7)
  set letterSpacing($core.double value) => $_setDouble(6, value);
  @$pb.TagNumber(7)
  $core.bool hasLetterSpacing() => $_has(6);
  @$pb.TagNumber(7)
  void clearLetterSpacing() => $_clearField(7);
}

/// Corners, borders, shadows.
class Shape extends $pb.GeneratedMessage {
  factory Shape({
    $core.int? cornerRadius,
    $core.int? borderWidth,
    $core.bool? elevation,
  }) {
    final result = create();
    if (cornerRadius != null) result.cornerRadius = cornerRadius;
    if (borderWidth != null) result.borderWidth = borderWidth;
    if (elevation != null) result.elevation = elevation;
    return result;
  }

  Shape._();

  factory Shape.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory Shape.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Shape',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aI(1, _omitFieldNames ? '' : 'cornerRadius',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(2, _omitFieldNames ? '' : 'borderWidth',
        fieldType: $pb.PbFieldType.OU3)
    ..aOB(3, _omitFieldNames ? '' : 'elevation')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Shape clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Shape copyWith(void Function(Shape) updates) =>
      super.copyWith((message) => updates(message as Shape)) as Shape;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static Shape create() => Shape._();
  @$core.override
  Shape createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static Shape getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<Shape>(create);
  static Shape? _defaultInstance;

  @$pb.TagNumber(1)
  $core.int get cornerRadius => $_getIZ(0);
  @$pb.TagNumber(1)
  set cornerRadius($core.int value) => $_setUnsignedInt32(0, value);
  @$pb.TagNumber(1)
  $core.bool hasCornerRadius() => $_has(0);
  @$pb.TagNumber(1)
  void clearCornerRadius() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get borderWidth => $_getIZ(1);
  @$pb.TagNumber(2)
  set borderWidth($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasBorderWidth() => $_has(1);
  @$pb.TagNumber(2)
  void clearBorderWidth() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.bool get elevation => $_getBF(2);
  @$pb.TagNumber(3)
  set elevation($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasElevation() => $_has(2);
  @$pb.TagNumber(3)
  void clearElevation() => $_clearField(3);
}

/// What the theme promises for accessibility; the validator enforces `contrast`.
class Accessibility extends $pb.GeneratedMessage {
  factory Accessibility({
    ContrastLevel? contrast,
    $core.int? focusRingWidth,
    $core.int? minTargetSize,
    $core.bool? reduceMotion,
    $core.bool? statusIcons,
  }) {
    final result = create();
    if (contrast != null) result.contrast = contrast;
    if (focusRingWidth != null) result.focusRingWidth = focusRingWidth;
    if (minTargetSize != null) result.minTargetSize = minTargetSize;
    if (reduceMotion != null) result.reduceMotion = reduceMotion;
    if (statusIcons != null) result.statusIcons = statusIcons;
    return result;
  }

  Accessibility._();

  factory Accessibility.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory Accessibility.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'Accessibility',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aE<ContrastLevel>(1, _omitFieldNames ? '' : 'contrast',
        enumValues: ContrastLevel.values)
    ..aI(2, _omitFieldNames ? '' : 'focusRingWidth',
        fieldType: $pb.PbFieldType.OU3)
    ..aI(3, _omitFieldNames ? '' : 'minTargetSize',
        fieldType: $pb.PbFieldType.OU3)
    ..aOB(4, _omitFieldNames ? '' : 'reduceMotion')
    ..aOB(5, _omitFieldNames ? '' : 'statusIcons')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Accessibility clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  Accessibility copyWith(void Function(Accessibility) updates) =>
      super.copyWith((message) => updates(message as Accessibility))
          as Accessibility;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static Accessibility create() => Accessibility._();
  @$core.override
  Accessibility createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static Accessibility getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<Accessibility>(create);
  static Accessibility? _defaultInstance;

  @$pb.TagNumber(1)
  ContrastLevel get contrast => $_getN(0);
  @$pb.TagNumber(1)
  set contrast(ContrastLevel value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasContrast() => $_has(0);
  @$pb.TagNumber(1)
  void clearContrast() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get focusRingWidth => $_getIZ(1);
  @$pb.TagNumber(2)
  set focusRingWidth($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasFocusRingWidth() => $_has(1);
  @$pb.TagNumber(2)
  void clearFocusRingWidth() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.int get minTargetSize => $_getIZ(2);
  @$pb.TagNumber(3)
  set minTargetSize($core.int value) => $_setUnsignedInt32(2, value);
  @$pb.TagNumber(3)
  $core.bool hasMinTargetSize() => $_has(2);
  @$pb.TagNumber(3)
  void clearMinTargetSize() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.bool get reduceMotion => $_getBF(3);
  @$pb.TagNumber(4)
  set reduceMotion($core.bool value) => $_setBool(3, value);
  @$pb.TagNumber(4)
  $core.bool hasReduceMotion() => $_has(3);
  @$pb.TagNumber(4)
  void clearReduceMotion() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.bool get statusIcons => $_getBF(4);
  @$pb.TagNumber(5)
  set statusIcons($core.bool value) => $_setBool(4, value);
  @$pb.TagNumber(5)
  $core.bool hasStatusIcons() => $_has(4);
  @$pb.TagNumber(5)
  void clearStatusIcons() => $_clearField(5);
}

class ListThemesRequest extends $pb.GeneratedMessage {
  factory ListThemesRequest() => create();

  ListThemesRequest._();

  factory ListThemesRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ListThemesRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ListThemesRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListThemesRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListThemesRequest copyWith(void Function(ListThemesRequest) updates) =>
      super.copyWith((message) => updates(message as ListThemesRequest))
          as ListThemesRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ListThemesRequest create() => ListThemesRequest._();
  @$core.override
  ListThemesRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ListThemesRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ListThemesRequest>(create);
  static ListThemesRequest? _defaultInstance;
}

class ListThemesResponse extends $pb.GeneratedMessage {
  factory ListThemesResponse({
    $core.Iterable<Theme>? themes,
  }) {
    final result = create();
    if (themes != null) result.themes.addAll(themes);
    return result;
  }

  ListThemesResponse._();

  factory ListThemesResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ListThemesResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ListThemesResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<Theme>(1, _omitFieldNames ? '' : 'themes', subBuilder: Theme.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListThemesResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListThemesResponse copyWith(void Function(ListThemesResponse) updates) =>
      super.copyWith((message) => updates(message as ListThemesResponse))
          as ListThemesResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ListThemesResponse create() => ListThemesResponse._();
  @$core.override
  ListThemesResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ListThemesResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ListThemesResponse>(create);
  static ListThemesResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<Theme> get themes => $_getList(0);
}

class GetThemeRequest extends $pb.GeneratedMessage {
  factory GetThemeRequest({
    $core.String? id,
  }) {
    final result = create();
    if (id != null) result.id = id;
    return result;
  }

  GetThemeRequest._();

  factory GetThemeRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GetThemeRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GetThemeRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetThemeRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetThemeRequest copyWith(void Function(GetThemeRequest) updates) =>
      super.copyWith((message) => updates(message as GetThemeRequest))
          as GetThemeRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GetThemeRequest create() => GetThemeRequest._();
  @$core.override
  GetThemeRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GetThemeRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GetThemeRequest>(create);
  static GetThemeRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);
}

class DeleteThemeRequest extends $pb.GeneratedMessage {
  factory DeleteThemeRequest({
    $core.String? id,
  }) {
    final result = create();
    if (id != null) result.id = id;
    return result;
  }

  DeleteThemeRequest._();

  factory DeleteThemeRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory DeleteThemeRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'DeleteThemeRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DeleteThemeRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DeleteThemeRequest copyWith(void Function(DeleteThemeRequest) updates) =>
      super.copyWith((message) => updates(message as DeleteThemeRequest))
          as DeleteThemeRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static DeleteThemeRequest create() => DeleteThemeRequest._();
  @$core.override
  DeleteThemeRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static DeleteThemeRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<DeleteThemeRequest>(create);
  static DeleteThemeRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);
}

class DeleteThemeResponse extends $pb.GeneratedMessage {
  factory DeleteThemeResponse({
    $core.bool? restoredBuiltin,
  }) {
    final result = create();
    if (restoredBuiltin != null) result.restoredBuiltin = restoredBuiltin;
    return result;
  }

  DeleteThemeResponse._();

  factory DeleteThemeResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory DeleteThemeResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'DeleteThemeResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'restoredBuiltin')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DeleteThemeResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DeleteThemeResponse copyWith(void Function(DeleteThemeResponse) updates) =>
      super.copyWith((message) => updates(message as DeleteThemeResponse))
          as DeleteThemeResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static DeleteThemeResponse create() => DeleteThemeResponse._();
  @$core.override
  DeleteThemeResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static DeleteThemeResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<DeleteThemeResponse>(create);
  static DeleteThemeResponse? _defaultInstance;

  /// Always false until operators can override a built-in.
  @$pb.TagNumber(1)
  $core.bool get restoredBuiltin => $_getBF(0);
  @$pb.TagNumber(1)
  set restoredBuiltin($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRestoredBuiltin() => $_has(0);
  @$pb.TagNumber(1)
  void clearRestoredBuiltin() => $_clearField(1);
}

class ActiveTheme extends $pb.GeneratedMessage {
  factory ActiveTheme({
    Theme? theme,
    ThemeMode? mode,
    ActiveSource? source,
  }) {
    final result = create();
    if (theme != null) result.theme = theme;
    if (mode != null) result.mode = mode;
    if (source != null) result.source = source;
    return result;
  }

  ActiveTheme._();

  factory ActiveTheme.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ActiveTheme.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ActiveTheme',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<Theme>(1, _omitFieldNames ? '' : 'theme', subBuilder: Theme.create)
    ..aE<ThemeMode>(2, _omitFieldNames ? '' : 'mode',
        enumValues: ThemeMode.values)
    ..aE<ActiveSource>(3, _omitFieldNames ? '' : 'source',
        enumValues: ActiveSource.values)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ActiveTheme clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ActiveTheme copyWith(void Function(ActiveTheme) updates) =>
      super.copyWith((message) => updates(message as ActiveTheme))
          as ActiveTheme;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ActiveTheme create() => ActiveTheme._();
  @$core.override
  ActiveTheme createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ActiveTheme getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ActiveTheme>(create);
  static ActiveTheme? _defaultInstance;

  @$pb.TagNumber(1)
  Theme get theme => $_getN(0);
  @$pb.TagNumber(1)
  set theme(Theme value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasTheme() => $_has(0);
  @$pb.TagNumber(1)
  void clearTheme() => $_clearField(1);
  @$pb.TagNumber(1)
  Theme ensureTheme() => $_ensure(0);

  @$pb.TagNumber(2)
  ThemeMode get mode => $_getN(1);
  @$pb.TagNumber(2)
  set mode(ThemeMode value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasMode() => $_has(1);
  @$pb.TagNumber(2)
  void clearMode() => $_clearField(2);

  @$pb.TagNumber(3)
  ActiveSource get source => $_getN(2);
  @$pb.TagNumber(3)
  set source(ActiveSource value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasSource() => $_has(2);
  @$pb.TagNumber(3)
  void clearSource() => $_clearField(3);
}

class GetActiveThemeRequest extends $pb.GeneratedMessage {
  factory GetActiveThemeRequest() => create();

  GetActiveThemeRequest._();

  factory GetActiveThemeRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GetActiveThemeRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GetActiveThemeRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetActiveThemeRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetActiveThemeRequest copyWith(
          void Function(GetActiveThemeRequest) updates) =>
      super.copyWith((message) => updates(message as GetActiveThemeRequest))
          as GetActiveThemeRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GetActiveThemeRequest create() => GetActiveThemeRequest._();
  @$core.override
  GetActiveThemeRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GetActiveThemeRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GetActiveThemeRequest>(create);
  static GetActiveThemeRequest? _defaultInstance;
}

class SetActiveThemeRequest extends $pb.GeneratedMessage {
  factory SetActiveThemeRequest({
    $core.String? themeId,
    ThemeMode? mode,
  }) {
    final result = create();
    if (themeId != null) result.themeId = themeId;
    if (mode != null) result.mode = mode;
    return result;
  }

  SetActiveThemeRequest._();

  factory SetActiveThemeRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory SetActiveThemeRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'SetActiveThemeRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'themeId')
    ..aE<ThemeMode>(2, _omitFieldNames ? '' : 'mode',
        enumValues: ThemeMode.values)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SetActiveThemeRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  SetActiveThemeRequest copyWith(
          void Function(SetActiveThemeRequest) updates) =>
      super.copyWith((message) => updates(message as SetActiveThemeRequest))
          as SetActiveThemeRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static SetActiveThemeRequest create() => SetActiveThemeRequest._();
  @$core.override
  SetActiveThemeRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static SetActiveThemeRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<SetActiveThemeRequest>(create);
  static SetActiveThemeRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get themeId => $_getSZ(0);
  @$pb.TagNumber(1)
  set themeId($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasThemeId() => $_has(0);
  @$pb.TagNumber(1)
  void clearThemeId() => $_clearField(1);

  @$pb.TagNumber(2)
  ThemeMode get mode => $_getN(1);
  @$pb.TagNumber(2)
  set mode(ThemeMode value) => $_setField(2, value);
  @$pb.TagNumber(2)
  $core.bool hasMode() => $_has(1);
  @$pb.TagNumber(2)
  void clearMode() => $_clearField(2);
}

/// A user's stored preference (collection `user_prefs`), keyed by a hash of the
/// verified subject inside the caller's tenant. Never sent by a client.
class UserPrefs extends $pb.GeneratedMessage {
  factory UserPrefs({
    $core.String? id,
    $core.String? themeId,
    ThemeMode? mode,
    $core.Iterable<$core.String>? privateThemeIds,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (themeId != null) result.themeId = themeId;
    if (mode != null) result.mode = mode;
    if (privateThemeIds != null) result.privateThemeIds.addAll(privateThemeIds);
    return result;
  }

  UserPrefs._();

  factory UserPrefs.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory UserPrefs.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'UserPrefs',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..aOS(2, _omitFieldNames ? '' : 'themeId')
    ..aE<ThemeMode>(3, _omitFieldNames ? '' : 'mode',
        enumValues: ThemeMode.values)
    ..pPS(4, _omitFieldNames ? '' : 'privateThemeIds')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UserPrefs clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  UserPrefs copyWith(void Function(UserPrefs) updates) =>
      super.copyWith((message) => updates(message as UserPrefs)) as UserPrefs;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static UserPrefs create() => UserPrefs._();
  @$core.override
  UserPrefs createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static UserPrefs getDefault() =>
      _defaultInstance ??= $pb.GeneratedMessage.$_defaultFor<UserPrefs>(create);
  static UserPrefs? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get themeId => $_getSZ(1);
  @$pb.TagNumber(2)
  set themeId($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasThemeId() => $_has(1);
  @$pb.TagNumber(2)
  void clearThemeId() => $_clearField(2);

  @$pb.TagNumber(3)
  ThemeMode get mode => $_getN(2);
  @$pb.TagNumber(3)
  set mode(ThemeMode value) => $_setField(3, value);
  @$pb.TagNumber(3)
  $core.bool hasMode() => $_has(2);
  @$pb.TagNumber(3)
  void clearMode() => $_clearField(3);

  /// Server-maintained: the ids of the user's private themes, updated in the same
  /// write as each private theme, so the per-owner cap needs no scan.
  @$pb.TagNumber(4)
  $pb.PbList<$core.String> get privateThemeIds => $_getList(3);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
