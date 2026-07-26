// This is a generated file - do not edit.
//
// Generated from agent/v1/reference.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

import 'common.pb.dart' as $1;

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

class RefResolveRequest extends $pb.GeneratedMessage {
  factory RefResolveRequest({
    $core.String? prompt,
    $fixnum.Int64? budgetTokens,
  }) {
    final result = create();
    if (prompt != null) result.prompt = prompt;
    if (budgetTokens != null) result.budgetTokens = budgetTokens;
    return result;
  }

  RefResolveRequest._();

  factory RefResolveRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RefResolveRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RefResolveRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'prompt')
    ..a<$fixnum.Int64>(
        2, _omitFieldNames ? '' : 'budgetTokens', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RefResolveRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RefResolveRequest copyWith(void Function(RefResolveRequest) updates) =>
      super.copyWith((message) => updates(message as RefResolveRequest))
          as RefResolveRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RefResolveRequest create() => RefResolveRequest._();
  @$core.override
  RefResolveRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RefResolveRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RefResolveRequest>(create);
  static RefResolveRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get prompt => $_getSZ(0);
  @$pb.TagNumber(1)
  set prompt($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasPrompt() => $_has(0);
  @$pb.TagNumber(1)
  void clearPrompt() => $_clearField(1);

  /// 0 ⇒ unbounded.
  @$pb.TagNumber(2)
  $fixnum.Int64 get budgetTokens => $_getI64(1);
  @$pb.TagNumber(2)
  set budgetTokens($fixnum.Int64 value) => $_setInt64(1, value);
  @$pb.TagNumber(2)
  $core.bool hasBudgetTokens() => $_has(1);
  @$pb.TagNumber(2)
  void clearBudgetTokens() => $_clearField(2);
}

class RefResolution extends $pb.GeneratedMessage {
  factory RefResolution({
    $core.Iterable<$1.ContextBlock>? blocks,
    $core.Iterable<$core.String>? warnings,
    $core.bool? blocked,
  }) {
    final result = create();
    if (blocks != null) result.blocks.addAll(blocks);
    if (warnings != null) result.warnings.addAll(warnings);
    if (blocked != null) result.blocked = blocked;
    return result;
  }

  RefResolution._();

  factory RefResolution.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RefResolution.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RefResolution',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<$1.ContextBlock>(1, _omitFieldNames ? '' : 'blocks',
        subBuilder: $1.ContextBlock.create)
    ..pPS(2, _omitFieldNames ? '' : 'warnings')
    ..aOB(3, _omitFieldNames ? '' : 'blocked')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RefResolution clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RefResolution copyWith(void Function(RefResolution) updates) =>
      super.copyWith((message) => updates(message as RefResolution))
          as RefResolution;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RefResolution create() => RefResolution._();
  @$core.override
  RefResolution createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RefResolution getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RefResolution>(create);
  static RefResolution? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<$1.ContextBlock> get blocks => $_getList(0);

  /// Unresolved / denied / injection-blocked / over-budget, for the operator.
  @$pb.TagNumber(2)
  $pb.PbList<$core.String> get warnings => $_getList(1);

  /// Expansion was refused wholesale (over the hard budget); the caller must
  /// leave the prompt unmodified.
  @$pb.TagNumber(3)
  $core.bool get blocked => $_getBF(2);
  @$pb.TagNumber(3)
  set blocked($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasBlocked() => $_has(2);
  @$pb.TagNumber(3)
  void clearBlocked() => $_clearField(3);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
