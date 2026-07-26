// This is a generated file - do not edit.
//
// Generated from agent/v1/review.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:protobuf/protobuf.dart' as $pb;

class ReviewCollectStatus extends $pb.ProtobufEnum {
  static const ReviewCollectStatus REVIEW_COLLECT_STATUS_UNSPECIFIED =
      ReviewCollectStatus._(
          0, _omitEnumNames ? '' : 'REVIEW_COLLECT_STATUS_UNSPECIFIED');
  static const ReviewCollectStatus REVIEW_COLLECT_STATUS_OK =
      ReviewCollectStatus._(
          1, _omitEnumNames ? '' : 'REVIEW_COLLECT_STATUS_OK');
  static const ReviewCollectStatus REVIEW_COLLECT_STATUS_PARTIAL =
      ReviewCollectStatus._(
          2, _omitEnumNames ? '' : 'REVIEW_COLLECT_STATUS_PARTIAL');
  static const ReviewCollectStatus REVIEW_COLLECT_STATUS_SKIPPED =
      ReviewCollectStatus._(
          3, _omitEnumNames ? '' : 'REVIEW_COLLECT_STATUS_SKIPPED');
  static const ReviewCollectStatus REVIEW_COLLECT_STATUS_FAILED =
      ReviewCollectStatus._(
          4, _omitEnumNames ? '' : 'REVIEW_COLLECT_STATUS_FAILED');

  static const $core.List<ReviewCollectStatus> values = <ReviewCollectStatus>[
    REVIEW_COLLECT_STATUS_UNSPECIFIED,
    REVIEW_COLLECT_STATUS_OK,
    REVIEW_COLLECT_STATUS_PARTIAL,
    REVIEW_COLLECT_STATUS_SKIPPED,
    REVIEW_COLLECT_STATUS_FAILED,
  ];

  static final $core.List<ReviewCollectStatus?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 4);
  static ReviewCollectStatus? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const ReviewCollectStatus._(super.value, super.name);
}

class ReviewForgeHost extends $pb.ProtobufEnum {
  static const ReviewForgeHost REVIEW_FORGE_HOST_UNSPECIFIED =
      ReviewForgeHost._(
          0, _omitEnumNames ? '' : 'REVIEW_FORGE_HOST_UNSPECIFIED');
  static const ReviewForgeHost REVIEW_FORGE_HOST_GITHUB =
      ReviewForgeHost._(1, _omitEnumNames ? '' : 'REVIEW_FORGE_HOST_GITHUB');
  static const ReviewForgeHost REVIEW_FORGE_HOST_GITLAB =
      ReviewForgeHost._(2, _omitEnumNames ? '' : 'REVIEW_FORGE_HOST_GITLAB');
  static const ReviewForgeHost REVIEW_FORGE_HOST_OTHER =
      ReviewForgeHost._(3, _omitEnumNames ? '' : 'REVIEW_FORGE_HOST_OTHER');
  static const ReviewForgeHost REVIEW_FORGE_HOST_NONE =
      ReviewForgeHost._(4, _omitEnumNames ? '' : 'REVIEW_FORGE_HOST_NONE');

  static const $core.List<ReviewForgeHost> values = <ReviewForgeHost>[
    REVIEW_FORGE_HOST_UNSPECIFIED,
    REVIEW_FORGE_HOST_GITHUB,
    REVIEW_FORGE_HOST_GITLAB,
    REVIEW_FORGE_HOST_OTHER,
    REVIEW_FORGE_HOST_NONE,
  ];

  static final $core.List<ReviewForgeHost?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 4);
  static ReviewForgeHost? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const ReviewForgeHost._(super.value, super.name);
}

class ReviewRepoRelation extends $pb.ProtobufEnum {
  static const ReviewRepoRelation REVIEW_REPO_RELATION_UNSPECIFIED =
      ReviewRepoRelation._(
          0, _omitEnumNames ? '' : 'REVIEW_REPO_RELATION_UNSPECIFIED');
  static const ReviewRepoRelation REVIEW_REPO_RELATION_CLONE =
      ReviewRepoRelation._(
          1, _omitEnumNames ? '' : 'REVIEW_REPO_RELATION_CLONE');
  static const ReviewRepoRelation REVIEW_REPO_RELATION_FORK =
      ReviewRepoRelation._(
          2, _omitEnumNames ? '' : 'REVIEW_REPO_RELATION_FORK');
  static const ReviewRepoRelation REVIEW_REPO_RELATION_UNKNOWN =
      ReviewRepoRelation._(
          3, _omitEnumNames ? '' : 'REVIEW_REPO_RELATION_UNKNOWN');

  static const $core.List<ReviewRepoRelation> values = <ReviewRepoRelation>[
    REVIEW_REPO_RELATION_UNSPECIFIED,
    REVIEW_REPO_RELATION_CLONE,
    REVIEW_REPO_RELATION_FORK,
    REVIEW_REPO_RELATION_UNKNOWN,
  ];

  static final $core.List<ReviewRepoRelation?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 3);
  static ReviewRepoRelation? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const ReviewRepoRelation._(super.value, super.name);
}

class ReviewRepoLanguage extends $pb.ProtobufEnum {
  static const ReviewRepoLanguage REVIEW_REPO_LANGUAGE_UNSPECIFIED =
      ReviewRepoLanguage._(
          0, _omitEnumNames ? '' : 'REVIEW_REPO_LANGUAGE_UNSPECIFIED');
  static const ReviewRepoLanguage REVIEW_REPO_LANGUAGE_GO =
      ReviewRepoLanguage._(1, _omitEnumNames ? '' : 'REVIEW_REPO_LANGUAGE_GO');
  static const ReviewRepoLanguage REVIEW_REPO_LANGUAGE_RUST =
      ReviewRepoLanguage._(
          2, _omitEnumNames ? '' : 'REVIEW_REPO_LANGUAGE_RUST');
  static const ReviewRepoLanguage REVIEW_REPO_LANGUAGE_MIXED =
      ReviewRepoLanguage._(
          3, _omitEnumNames ? '' : 'REVIEW_REPO_LANGUAGE_MIXED');
  static const ReviewRepoLanguage REVIEW_REPO_LANGUAGE_UNKNOWN =
      ReviewRepoLanguage._(
          4, _omitEnumNames ? '' : 'REVIEW_REPO_LANGUAGE_UNKNOWN');

  static const $core.List<ReviewRepoLanguage> values = <ReviewRepoLanguage>[
    REVIEW_REPO_LANGUAGE_UNSPECIFIED,
    REVIEW_REPO_LANGUAGE_GO,
    REVIEW_REPO_LANGUAGE_RUST,
    REVIEW_REPO_LANGUAGE_MIXED,
    REVIEW_REPO_LANGUAGE_UNKNOWN,
  ];

  static final $core.List<ReviewRepoLanguage?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 4);
  static ReviewRepoLanguage? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const ReviewRepoLanguage._(super.value, super.name);
}

const $core.bool _omitEnumNames =
    $core.bool.fromEnvironment('protobuf.omit_enum_names');
