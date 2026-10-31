// This is a generated file - do not edit.
//
// Generated from agent/v1/theme.proto.

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

@$core.Deprecated('Use densityDescriptor instead')
const Density$json = {
  '1': 'Density',
  '2': [
    {'1': 'DENSITY_UNSPECIFIED', '2': 0},
    {'1': 'DENSITY_COMPACT', '2': 1},
    {'1': 'DENSITY_STANDARD', '2': 2},
    {'1': 'DENSITY_COMFORTABLE', '2': 3},
  ],
};

/// Descriptor for `Density`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List densityDescriptor = $convert.base64Decode(
    'CgdEZW5zaXR5EhcKE0RFTlNJVFlfVU5TUEVDSUZJRUQQABITCg9ERU5TSVRZX0NPTVBBQ1QQAR'
    'IUChBERU5TSVRZX1NUQU5EQVJEEAISFwoTREVOU0lUWV9DT01GT1JUQUJMRRAD');

@$core.Deprecated('Use contrastLevelDescriptor instead')
const ContrastLevel$json = {
  '1': 'ContrastLevel',
  '2': [
    {'1': 'CONTRAST_LEVEL_UNSPECIFIED', '2': 0},
    {'1': 'CONTRAST_LEVEL_AA', '2': 1},
    {'1': 'CONTRAST_LEVEL_AAA', '2': 2},
  ],
};

/// Descriptor for `ContrastLevel`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List contrastLevelDescriptor = $convert.base64Decode(
    'Cg1Db250cmFzdExldmVsEh4KGkNPTlRSQVNUX0xFVkVMX1VOU1BFQ0lGSUVEEAASFQoRQ09OVF'
    'JBU1RfTEVWRUxfQUEQARIWChJDT05UUkFTVF9MRVZFTF9BQUEQAg==');

@$core.Deprecated('Use visibilityDescriptor instead')
const Visibility$json = {
  '1': 'Visibility',
  '2': [
    {'1': 'VISIBILITY_UNSPECIFIED', '2': 0},
    {'1': 'VISIBILITY_PRIVATE', '2': 1},
    {'1': 'VISIBILITY_TENANT', '2': 2},
  ],
};

/// Descriptor for `Visibility`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List visibilityDescriptor = $convert.base64Decode(
    'CgpWaXNpYmlsaXR5EhoKFlZJU0lCSUxJVFlfVU5TUEVDSUZJRUQQABIWChJWSVNJQklMSVRZX1'
    'BSSVZBVEUQARIVChFWSVNJQklMSVRZX1RFTkFOVBAC');

@$core.Deprecated('Use themeModeDescriptor instead')
const ThemeMode$json = {
  '1': 'ThemeMode',
  '2': [
    {'1': 'THEME_MODE_UNSPECIFIED', '2': 0},
    {'1': 'THEME_MODE_SYSTEM', '2': 1},
    {'1': 'THEME_MODE_LIGHT', '2': 2},
    {'1': 'THEME_MODE_DARK', '2': 3},
  ],
};

/// Descriptor for `ThemeMode`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List themeModeDescriptor = $convert.base64Decode(
    'CglUaGVtZU1vZGUSGgoWVEhFTUVfTU9ERV9VTlNQRUNJRklFRBAAEhUKEVRIRU1FX01PREVfU1'
    'lTVEVNEAESFAoQVEhFTUVfTU9ERV9MSUdIVBACEhMKD1RIRU1FX01PREVfREFSSxAD');

@$core.Deprecated('Use activeSourceDescriptor instead')
const ActiveSource$json = {
  '1': 'ActiveSource',
  '2': [
    {'1': 'ACTIVE_SOURCE_UNSPECIFIED', '2': 0},
    {'1': 'ACTIVE_SOURCE_USER', '2': 1},
    {'1': 'ACTIVE_SOURCE_SERVER_DEFAULT', '2': 2},
    {'1': 'ACTIVE_SOURCE_BUILTIN', '2': 3},
  ],
};

/// Descriptor for `ActiveSource`. Decode as a `google.protobuf.EnumDescriptorProto`.
final $typed_data.Uint8List activeSourceDescriptor = $convert.base64Decode(
    'CgxBY3RpdmVTb3VyY2USHQoZQUNUSVZFX1NPVVJDRV9VTlNQRUNJRklFRBAAEhYKEkFDVElWRV'
    '9TT1VSQ0VfVVNFUhABEiAKHEFDVElWRV9TT1VSQ0VfU0VSVkVSX0RFRkFVTFQQAhIZChVBQ1RJ'
    'VkVfU09VUkNFX0JVSUxUSU4QAw==');

@$core.Deprecated('Use themeDescriptor instead')
const Theme$json = {
  '1': 'Theme',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
    {'1': 'name', '3': 2, '4': 1, '5': 9, '10': 'name'},
    {'1': 'description', '3': 3, '4': 1, '5': 9, '10': 'description'},
    {'1': 'author', '3': 4, '4': 1, '5': 9, '10': 'author'},
    {'1': 'builtin', '3': 5, '4': 1, '5': 8, '10': 'builtin'},
    {'1': 'version', '3': 6, '4': 1, '5': 4, '10': 'version'},
    {
      '1': 'light',
      '3': 7,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.Palette',
      '10': 'light'
    },
    {
      '1': 'dark',
      '3': 8,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.Palette',
      '10': 'dark'
    },
    {
      '1': 'typography',
      '3': 9,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.Typography',
      '10': 'typography'
    },
    {
      '1': 'shape',
      '3': 10,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.Shape',
      '10': 'shape'
    },
    {
      '1': 'density',
      '3': 11,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.Density',
      '10': 'density'
    },
    {
      '1': 'accessibility',
      '3': 12,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.Accessibility',
      '10': 'accessibility'
    },
    {
      '1': 'visibility',
      '3': 13,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.Visibility',
      '10': 'visibility'
    },
    {'1': 'owner', '3': 14, '4': 1, '5': 9, '10': 'owner'},
  ],
};

/// Descriptor for `Theme`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List themeDescriptor = $convert.base64Decode(
    'CgVUaGVtZRIOCgJpZBgBIAEoCVICaWQSEgoEbmFtZRgCIAEoCVIEbmFtZRIgCgtkZXNjcmlwdG'
    'lvbhgDIAEoCVILZGVzY3JpcHRpb24SFgoGYXV0aG9yGAQgASgJUgZhdXRob3ISGAoHYnVpbHRp'
    'bhgFIAEoCFIHYnVpbHRpbhIYCgd2ZXJzaW9uGAYgASgEUgd2ZXJzaW9uEicKBWxpZ2h0GAcgAS'
    'gLMhEuYWdlbnQudjEuUGFsZXR0ZVIFbGlnaHQSJQoEZGFyaxgIIAEoCzIRLmFnZW50LnYxLlBh'
    'bGV0dGVSBGRhcmsSNAoKdHlwb2dyYXBoeRgJIAEoCzIULmFnZW50LnYxLlR5cG9ncmFwaHlSCn'
    'R5cG9ncmFwaHkSJQoFc2hhcGUYCiABKAsyDy5hZ2VudC52MS5TaGFwZVIFc2hhcGUSKwoHZGVu'
    'c2l0eRgLIAEoDjIRLmFnZW50LnYxLkRlbnNpdHlSB2RlbnNpdHkSPQoNYWNjZXNzaWJpbGl0eR'
    'gMIAEoCzIXLmFnZW50LnYxLkFjY2Vzc2liaWxpdHlSDWFjY2Vzc2liaWxpdHkSNAoKdmlzaWJp'
    'bGl0eRgNIAEoDjIULmFnZW50LnYxLlZpc2liaWxpdHlSCnZpc2liaWxpdHkSFAoFb3duZXIYDi'
    'ABKAlSBW93bmVy');

@$core.Deprecated('Use paletteDescriptor instead')
const Palette$json = {
  '1': 'Palette',
  '2': [
    {'1': 'seed', '3': 1, '4': 1, '5': 9, '10': 'seed'},
    {'1': 'primary', '3': 2, '4': 1, '5': 9, '10': 'primary'},
    {'1': 'on_primary', '3': 3, '4': 1, '5': 9, '10': 'onPrimary'},
    {'1': 'secondary', '3': 4, '4': 1, '5': 9, '10': 'secondary'},
    {'1': 'on_secondary', '3': 5, '4': 1, '5': 9, '10': 'onSecondary'},
    {'1': 'surface', '3': 6, '4': 1, '5': 9, '10': 'surface'},
    {'1': 'on_surface', '3': 7, '4': 1, '5': 9, '10': 'onSurface'},
    {'1': 'background', '3': 8, '4': 1, '5': 9, '10': 'background'},
    {'1': 'surface_variant', '3': 9, '4': 1, '5': 9, '10': 'surfaceVariant'},
    {
      '1': 'on_surface_variant',
      '3': 10,
      '4': 1,
      '5': 9,
      '10': 'onSurfaceVariant'
    },
    {'1': 'outline', '3': 11, '4': 1, '5': 9, '10': 'outline'},
    {'1': 'error', '3': 12, '4': 1, '5': 9, '10': 'error'},
    {'1': 'on_error', '3': 13, '4': 1, '5': 9, '10': 'onError'},
    {'1': 'focus', '3': 14, '4': 1, '5': 9, '10': 'focus'},
    {
      '1': 'semantic',
      '3': 15,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.SemanticColors',
      '10': 'semantic'
    },
  ],
};

/// Descriptor for `Palette`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List paletteDescriptor = $convert.base64Decode(
    'CgdQYWxldHRlEhIKBHNlZWQYASABKAlSBHNlZWQSGAoHcHJpbWFyeRgCIAEoCVIHcHJpbWFyeR'
    'IdCgpvbl9wcmltYXJ5GAMgASgJUglvblByaW1hcnkSHAoJc2Vjb25kYXJ5GAQgASgJUglzZWNv'
    'bmRhcnkSIQoMb25fc2Vjb25kYXJ5GAUgASgJUgtvblNlY29uZGFyeRIYCgdzdXJmYWNlGAYgAS'
    'gJUgdzdXJmYWNlEh0KCm9uX3N1cmZhY2UYByABKAlSCW9uU3VyZmFjZRIeCgpiYWNrZ3JvdW5k'
    'GAggASgJUgpiYWNrZ3JvdW5kEicKD3N1cmZhY2VfdmFyaWFudBgJIAEoCVIOc3VyZmFjZVZhcm'
    'lhbnQSLAoSb25fc3VyZmFjZV92YXJpYW50GAogASgJUhBvblN1cmZhY2VWYXJpYW50EhgKB291'
    'dGxpbmUYCyABKAlSB291dGxpbmUSFAoFZXJyb3IYDCABKAlSBWVycm9yEhkKCG9uX2Vycm9yGA'
    '0gASgJUgdvbkVycm9yEhQKBWZvY3VzGA4gASgJUgVmb2N1cxI0CghzZW1hbnRpYxgPIAEoCzIY'
    'LmFnZW50LnYxLlNlbWFudGljQ29sb3JzUghzZW1hbnRpYw==');

@$core.Deprecated('Use semanticColorsDescriptor instead')
const SemanticColors$json = {
  '1': 'SemanticColors',
  '2': [
    {'1': 'success', '3': 1, '4': 1, '5': 9, '10': 'success'},
    {'1': 'warning', '3': 2, '4': 1, '5': 9, '10': 'warning'},
    {'1': 'info', '3': 3, '4': 1, '5': 9, '10': 'info'},
    {'1': 'muted', '3': 4, '4': 1, '5': 9, '10': 'muted'},
    {'1': 'code_background', '3': 5, '4': 1, '5': 9, '10': 'codeBackground'},
    {'1': 'code_foreground', '3': 6, '4': 1, '5': 9, '10': 'codeForeground'},
    {
      '1': 'nav_tint',
      '3': 7,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.SemanticColors.NavTintEntry',
      '10': 'navTint'
    },
    {
      '1': 'graph_edge',
      '3': 8,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.SemanticColors.GraphEdgeEntry',
      '10': 'graphEdge'
    },
  ],
  '3': [SemanticColors_NavTintEntry$json, SemanticColors_GraphEdgeEntry$json],
};

@$core.Deprecated('Use semanticColorsDescriptor instead')
const SemanticColors_NavTintEntry$json = {
  '1': 'NavTintEntry',
  '2': [
    {'1': 'key', '3': 1, '4': 1, '5': 9, '10': 'key'},
    {'1': 'value', '3': 2, '4': 1, '5': 9, '10': 'value'},
  ],
  '7': {'7': true},
};

@$core.Deprecated('Use semanticColorsDescriptor instead')
const SemanticColors_GraphEdgeEntry$json = {
  '1': 'GraphEdgeEntry',
  '2': [
    {'1': 'key', '3': 1, '4': 1, '5': 9, '10': 'key'},
    {'1': 'value', '3': 2, '4': 1, '5': 9, '10': 'value'},
  ],
  '7': {'7': true},
};

/// Descriptor for `SemanticColors`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List semanticColorsDescriptor = $convert.base64Decode(
    'Cg5TZW1hbnRpY0NvbG9ycxIYCgdzdWNjZXNzGAEgASgJUgdzdWNjZXNzEhgKB3dhcm5pbmcYAi'
    'ABKAlSB3dhcm5pbmcSEgoEaW5mbxgDIAEoCVIEaW5mbxIUCgVtdXRlZBgEIAEoCVIFbXV0ZWQS'
    'JwoPY29kZV9iYWNrZ3JvdW5kGAUgASgJUg5jb2RlQmFja2dyb3VuZBInCg9jb2RlX2ZvcmVncm'
    '91bmQYBiABKAlSDmNvZGVGb3JlZ3JvdW5kEkAKCG5hdl90aW50GAcgAygLMiUuYWdlbnQudjEu'
    'U2VtYW50aWNDb2xvcnMuTmF2VGludEVudHJ5UgduYXZUaW50EkYKCmdyYXBoX2VkZ2UYCCADKA'
    'syJy5hZ2VudC52MS5TZW1hbnRpY0NvbG9ycy5HcmFwaEVkZ2VFbnRyeVIJZ3JhcGhFZGdlGjoK'
    'DE5hdlRpbnRFbnRyeRIQCgNrZXkYASABKAlSA2tleRIUCgV2YWx1ZRgCIAEoCVIFdmFsdWU6Aj'
    'gBGjwKDkdyYXBoRWRnZUVudHJ5EhAKA2tleRgBIAEoCVIDa2V5EhQKBXZhbHVlGAIgASgJUgV2'
    'YWx1ZToCOAE=');

@$core.Deprecated('Use typographyDescriptor instead')
const Typography$json = {
  '1': 'Typography',
  '2': [
    {'1': 'body_family', '3': 1, '4': 1, '5': 9, '10': 'bodyFamily'},
    {'1': 'heading_family', '3': 2, '4': 1, '5': 9, '10': 'headingFamily'},
    {'1': 'mono_family', '3': 3, '4': 1, '5': 9, '10': 'monoFamily'},
    {'1': 'scale', '3': 4, '4': 1, '5': 1, '10': 'scale'},
    {'1': 'body_weight', '3': 5, '4': 1, '5': 13, '10': 'bodyWeight'},
    {'1': 'heading_weight', '3': 6, '4': 1, '5': 13, '10': 'headingWeight'},
    {'1': 'letter_spacing', '3': 7, '4': 1, '5': 1, '10': 'letterSpacing'},
  ],
};

/// Descriptor for `Typography`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List typographyDescriptor = $convert.base64Decode(
    'CgpUeXBvZ3JhcGh5Eh8KC2JvZHlfZmFtaWx5GAEgASgJUgpib2R5RmFtaWx5EiUKDmhlYWRpbm'
    'dfZmFtaWx5GAIgASgJUg1oZWFkaW5nRmFtaWx5Eh8KC21vbm9fZmFtaWx5GAMgASgJUgptb25v'
    'RmFtaWx5EhQKBXNjYWxlGAQgASgBUgVzY2FsZRIfCgtib2R5X3dlaWdodBgFIAEoDVIKYm9keV'
    'dlaWdodBIlCg5oZWFkaW5nX3dlaWdodBgGIAEoDVINaGVhZGluZ1dlaWdodBIlCg5sZXR0ZXJf'
    'c3BhY2luZxgHIAEoAVINbGV0dGVyU3BhY2luZw==');

@$core.Deprecated('Use shapeDescriptor instead')
const Shape$json = {
  '1': 'Shape',
  '2': [
    {'1': 'corner_radius', '3': 1, '4': 1, '5': 13, '10': 'cornerRadius'},
    {'1': 'border_width', '3': 2, '4': 1, '5': 13, '10': 'borderWidth'},
    {'1': 'elevation', '3': 3, '4': 1, '5': 8, '10': 'elevation'},
  ],
};

/// Descriptor for `Shape`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List shapeDescriptor = $convert.base64Decode(
    'CgVTaGFwZRIjCg1jb3JuZXJfcmFkaXVzGAEgASgNUgxjb3JuZXJSYWRpdXMSIQoMYm9yZGVyX3'
    'dpZHRoGAIgASgNUgtib3JkZXJXaWR0aBIcCgllbGV2YXRpb24YAyABKAhSCWVsZXZhdGlvbg==');

@$core.Deprecated('Use accessibilityDescriptor instead')
const Accessibility$json = {
  '1': 'Accessibility',
  '2': [
    {
      '1': 'contrast',
      '3': 1,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.ContrastLevel',
      '10': 'contrast'
    },
    {'1': 'focus_ring_width', '3': 2, '4': 1, '5': 13, '10': 'focusRingWidth'},
    {'1': 'min_target_size', '3': 3, '4': 1, '5': 13, '10': 'minTargetSize'},
    {'1': 'reduce_motion', '3': 4, '4': 1, '5': 8, '10': 'reduceMotion'},
    {'1': 'status_icons', '3': 5, '4': 1, '5': 8, '10': 'statusIcons'},
  ],
};

/// Descriptor for `Accessibility`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List accessibilityDescriptor = $convert.base64Decode(
    'Cg1BY2Nlc3NpYmlsaXR5EjMKCGNvbnRyYXN0GAEgASgOMhcuYWdlbnQudjEuQ29udHJhc3RMZX'
    'ZlbFIIY29udHJhc3QSKAoQZm9jdXNfcmluZ193aWR0aBgCIAEoDVIOZm9jdXNSaW5nV2lkdGgS'
    'JgoPbWluX3RhcmdldF9zaXplGAMgASgNUg1taW5UYXJnZXRTaXplEiMKDXJlZHVjZV9tb3Rpb2'
    '4YBCABKAhSDHJlZHVjZU1vdGlvbhIhCgxzdGF0dXNfaWNvbnMYBSABKAhSC3N0YXR1c0ljb25z');

@$core.Deprecated('Use listThemesRequestDescriptor instead')
const ListThemesRequest$json = {
  '1': 'ListThemesRequest',
};

/// Descriptor for `ListThemesRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List listThemesRequestDescriptor =
    $convert.base64Decode('ChFMaXN0VGhlbWVzUmVxdWVzdA==');

@$core.Deprecated('Use listThemesResponseDescriptor instead')
const ListThemesResponse$json = {
  '1': 'ListThemesResponse',
  '2': [
    {
      '1': 'themes',
      '3': 1,
      '4': 3,
      '5': 11,
      '6': '.agent.v1.Theme',
      '10': 'themes'
    },
  ],
};

/// Descriptor for `ListThemesResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List listThemesResponseDescriptor = $convert.base64Decode(
    'ChJMaXN0VGhlbWVzUmVzcG9uc2USJwoGdGhlbWVzGAEgAygLMg8uYWdlbnQudjEuVGhlbWVSBn'
    'RoZW1lcw==');

@$core.Deprecated('Use getThemeRequestDescriptor instead')
const GetThemeRequest$json = {
  '1': 'GetThemeRequest',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
  ],
};

/// Descriptor for `GetThemeRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List getThemeRequestDescriptor =
    $convert.base64Decode('Cg9HZXRUaGVtZVJlcXVlc3QSDgoCaWQYASABKAlSAmlk');

@$core.Deprecated('Use deleteThemeRequestDescriptor instead')
const DeleteThemeRequest$json = {
  '1': 'DeleteThemeRequest',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
  ],
};

/// Descriptor for `DeleteThemeRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List deleteThemeRequestDescriptor =
    $convert.base64Decode('ChJEZWxldGVUaGVtZVJlcXVlc3QSDgoCaWQYASABKAlSAmlk');

@$core.Deprecated('Use deleteThemeResponseDescriptor instead')
const DeleteThemeResponse$json = {
  '1': 'DeleteThemeResponse',
  '2': [
    {'1': 'restored_builtin', '3': 1, '4': 1, '5': 8, '10': 'restoredBuiltin'},
  ],
};

/// Descriptor for `DeleteThemeResponse`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List deleteThemeResponseDescriptor = $convert.base64Decode(
    'ChNEZWxldGVUaGVtZVJlc3BvbnNlEikKEHJlc3RvcmVkX2J1aWx0aW4YASABKAhSD3Jlc3Rvcm'
    'VkQnVpbHRpbg==');

@$core.Deprecated('Use activeThemeDescriptor instead')
const ActiveTheme$json = {
  '1': 'ActiveTheme',
  '2': [
    {
      '1': 'theme',
      '3': 1,
      '4': 1,
      '5': 11,
      '6': '.agent.v1.Theme',
      '10': 'theme'
    },
    {
      '1': 'mode',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.ThemeMode',
      '10': 'mode'
    },
    {
      '1': 'source',
      '3': 3,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.ActiveSource',
      '10': 'source'
    },
  ],
};

/// Descriptor for `ActiveTheme`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List activeThemeDescriptor = $convert.base64Decode(
    'CgtBY3RpdmVUaGVtZRIlCgV0aGVtZRgBIAEoCzIPLmFnZW50LnYxLlRoZW1lUgV0aGVtZRInCg'
    'Rtb2RlGAIgASgOMhMuYWdlbnQudjEuVGhlbWVNb2RlUgRtb2RlEi4KBnNvdXJjZRgDIAEoDjIW'
    'LmFnZW50LnYxLkFjdGl2ZVNvdXJjZVIGc291cmNl');

@$core.Deprecated('Use getActiveThemeRequestDescriptor instead')
const GetActiveThemeRequest$json = {
  '1': 'GetActiveThemeRequest',
};

/// Descriptor for `GetActiveThemeRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List getActiveThemeRequestDescriptor =
    $convert.base64Decode('ChVHZXRBY3RpdmVUaGVtZVJlcXVlc3Q=');

@$core.Deprecated('Use setActiveThemeRequestDescriptor instead')
const SetActiveThemeRequest$json = {
  '1': 'SetActiveThemeRequest',
  '2': [
    {'1': 'theme_id', '3': 1, '4': 1, '5': 9, '10': 'themeId'},
    {
      '1': 'mode',
      '3': 2,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.ThemeMode',
      '10': 'mode'
    },
  ],
};

/// Descriptor for `SetActiveThemeRequest`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List setActiveThemeRequestDescriptor = $convert.base64Decode(
    'ChVTZXRBY3RpdmVUaGVtZVJlcXVlc3QSGQoIdGhlbWVfaWQYASABKAlSB3RoZW1lSWQSJwoEbW'
    '9kZRgCIAEoDjITLmFnZW50LnYxLlRoZW1lTW9kZVIEbW9kZQ==');

@$core.Deprecated('Use userPrefsDescriptor instead')
const UserPrefs$json = {
  '1': 'UserPrefs',
  '2': [
    {'1': 'id', '3': 1, '4': 1, '5': 9, '10': 'id'},
    {'1': 'theme_id', '3': 2, '4': 1, '5': 9, '10': 'themeId'},
    {
      '1': 'mode',
      '3': 3,
      '4': 1,
      '5': 14,
      '6': '.agent.v1.ThemeMode',
      '10': 'mode'
    },
  ],
};

/// Descriptor for `UserPrefs`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List userPrefsDescriptor = $convert.base64Decode(
    'CglVc2VyUHJlZnMSDgoCaWQYASABKAlSAmlkEhkKCHRoZW1lX2lkGAIgASgJUgd0aGVtZUlkEi'
    'cKBG1vZGUYAyABKA4yEy5hZ2VudC52MS5UaGVtZU1vZGVSBG1vZGU=');
