// This is a generated file - do not edit.
//
// Generated from agent/v1/metrics_proxy.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports
// ignore_for_file: unused_import

import 'dart:convert' as $convert;
import 'dart:core' as $core;
import 'dart:typed_data' as $typed_data;

@$core.Deprecated('Use promQueryDescriptor instead')
const PromQuery$json = {
  '1': 'PromQuery',
  '2': [
    {'1': 'query', '3': 1, '4': 1, '5': 9, '10': 'query'},
    {
      '1': 'time_unix_ms',
      '3': 2,
      '4': 1,
      '5': 3,
      '9': 0,
      '10': 'timeUnixMs',
      '17': true
    },
  ],
  '8': [
    {'1': '_time_unix_ms'},
  ],
};

/// Descriptor for `PromQuery`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List promQueryDescriptor = $convert.base64Decode(
    'CglQcm9tUXVlcnkSFAoFcXVlcnkYASABKAlSBXF1ZXJ5EiUKDHRpbWVfdW5peF9tcxgCIAEoA0'
    'gAUgp0aW1lVW5peE1ziAEBQg8KDV90aW1lX3VuaXhfbXM=');

@$core.Deprecated('Use promRangeQueryDescriptor instead')
const PromRangeQuery$json = {
  '1': 'PromRangeQuery',
  '2': [
    {'1': 'query', '3': 1, '4': 1, '5': 9, '10': 'query'},
    {'1': 'start_unix_ms', '3': 2, '4': 1, '5': 3, '10': 'startUnixMs'},
    {'1': 'end_unix_ms', '3': 3, '4': 1, '5': 3, '10': 'endUnixMs'},
    {'1': 'step_secs', '3': 4, '4': 1, '5': 13, '10': 'stepSecs'},
  ],
};

/// Descriptor for `PromRangeQuery`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List promRangeQueryDescriptor = $convert.base64Decode(
    'Cg5Qcm9tUmFuZ2VRdWVyeRIUCgVxdWVyeRgBIAEoCVIFcXVlcnkSIgoNc3RhcnRfdW5peF9tcx'
    'gCIAEoA1ILc3RhcnRVbml4TXMSHgoLZW5kX3VuaXhfbXMYAyABKANSCWVuZFVuaXhNcxIbCglz'
    'dGVwX3NlY3MYBCABKA1SCHN0ZXBTZWNz');

@$core.Deprecated('Use promSampleDescriptor instead')
const PromSample$json = {
  '1': 'PromSample',
  '2': [
    {'1': 't_unix_ms', '3': 1, '4': 1, '5': 3, '10': 'tUnixMs'},
    {'1': 'value', '3': 2, '4': 1, '5': 1, '10': 'value'},
  ],
};

/// Descriptor for `PromSample`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List promSampleDescriptor = $convert.base64Decode(
    'CgpQcm9tU2FtcGxlEhoKCXRfdW5peF9tcxgBIAEoA1IHdFVuaXhNcxIUCgV2YWx1ZRgCIAEoAV'
    'IFdmFsdWU=');

@$core.Deprecated('Use promSeriesDescriptor instead')
const PromSeries$json = {
  '1': 'PromSeries',
  '2': [
    {
      '1': 'labels',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.PromSeries.LabelsEntry',
      '10': 'labels'
    },
    {
      '1': 'samples',
      '3': 2,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.PromSample',
      '10': 'samples'
    },
  ],
  '3': [PromSeries_LabelsEntry$json],
};

@$core.Deprecated('Use promSeriesDescriptor instead')
const PromSeries_LabelsEntry$json = {
  '1': 'LabelsEntry',
  '2': [
    {'1': 'key', '3': 1, '4': 1, '5': 9, '10': 'key'},
    {'1': 'value', '3': 2, '4': 1, '5': 9, '10': 'value'},
  ],
  '7': {'7': true},
};

/// Descriptor for `PromSeries`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List promSeriesDescriptor = $convert.base64Decode(
    'CgpQcm9tU2VyaWVzEjgKBmxhYmVscxgBIAMoCzIgLmFnZW50LnYxLlByb21TZXJpZXMuTGFiZW'
    'xzRW50cnlSBmxhYmVscxIuCgdzYW1wbGVzGAIgAygLMhQuYWdlbnQudjEuUHJvbVNhbXBsZVIH'
    'c2FtcGxlcxo5CgtMYWJlbHNFbnRyeRIQCgNrZXkYASABKAlSA2tleRIUCgV2YWx1ZRgCIAEoCV'
    'IFdmFsdWU6AjgB');

@$core.Deprecated('Use promResultDescriptor instead')
const PromResult$json = {
  '1': 'PromResult',
  '2': [
    {'1': 'result_type', '3': 1, '4': 1, '5': 9, '10': 'resultType'},
    {
      '1': 'series',
      '3': 2,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.PromSeries',
      '10': 'series'
    },
    {'1': 'error', '3': 3, '4': 1, '5': 9, '10': 'error'},
  ],
};

/// Descriptor for `PromResult`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List promResultDescriptor = $convert.base64Decode(
    'CgpQcm9tUmVzdWx0Eh8KC3Jlc3VsdF90eXBlGAEgASgJUgpyZXN1bHRUeXBlEiwKBnNlcmllcx'
    'gCIAMoCzIULmFnZW50LnYxLlByb21TZXJpZXNSBnNlcmllcxIUCgVlcnJvchgDIAEoCVIFZXJy'
    'b3I=');
