// This is a generated file - do not edit.
//
// Generated from agent/v1/auth.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

class ExchangeRequest extends $pb.GeneratedMessage {
  factory ExchangeRequest({
    $core.String? idToken,
    $core.String? clientKind,
    $core.bool? useClientCert,
    $core.String? code,
    $core.String? state,
    $core.String? codeVerifier,
  }) {
    final result = create();
    if (idToken != null) result.idToken = idToken;
    if (clientKind != null) result.clientKind = clientKind;
    if (useClientCert != null) result.useClientCert = useClientCert;
    if (code != null) result.code = code;
    if (state != null) result.state = state;
    if (codeVerifier != null) result.codeVerifier = codeVerifier;
    return result;
  }

  ExchangeRequest._();

  factory ExchangeRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ExchangeRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ExchangeRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'idToken')
    ..aOS(2, _omitFieldNames ? '' : 'clientKind')
    ..aOB(3, _omitFieldNames ? '' : 'useClientCert')
    ..aOS(4, _omitFieldNames ? '' : 'code')
    ..aOS(5, _omitFieldNames ? '' : 'state')
    ..aOS(6, _omitFieldNames ? '' : 'codeVerifier')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ExchangeRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ExchangeRequest copyWith(void Function(ExchangeRequest) updates) =>
      super.copyWith((message) => updates(message as ExchangeRequest))
          as ExchangeRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ExchangeRequest create() => ExchangeRequest._();
  @$core.override
  ExchangeRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ExchangeRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ExchangeRequest>(create);
  static ExchangeRequest? _defaultInstance;

  /// An OIDC ID token (compact JWT) from a configured issuer. At most 16 KiB.
  @$pb.TagNumber(1)
  $core.String get idToken => $_getSZ(0);
  @$pb.TagNumber(1)
  set idToken($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasIdToken() => $_has(0);
  @$pb.TagNumber(1)
  void clearIdToken() => $_clearField(1);

  /// `portal` | `cli` | `service`; anything else is recorded as `unspecified`.
  @$pb.TagNumber(2)
  $core.String get clientKind => $_getSZ(1);
  @$pb.TagNumber(2)
  set clientKind($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasClientKind() => $_has(1);
  @$pb.TagNumber(2)
  void clearClientKind() => $_clearField(2);

  /// Trade this connection's client certificate for a service token instead of
  /// an ID token (security-hardening S10). Needs a mutual-TLS listener and a
  /// certificate whose URI SAN is in `[auth.mtls] bindings`; `id_token` must be
  /// empty. The token is bound to the certificate (`cnf`), carries
  /// `sub = svc:<name>` and `amr = ["mtls"]`, and has no refresh handle: exchange
  /// again before it expires.
  @$pb.TagNumber(3)
  $core.bool get useClientCert => $_getBF(2);
  @$pb.TagNumber(3)
  set useClientCert($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasUseClientCert() => $_has(2);
  @$pb.TagNumber(3)
  void clearUseClientCert() => $_clearField(3);

  /// A browser sign-in's authorization code (S13), from the IdP's redirect. Needs
  /// `state` and `code_verifier`; `id_token` must be empty. At most 4 KiB.
  @$pb.TagNumber(4)
  $core.String get code => $_getSZ(3);
  @$pb.TagNumber(4)
  set code($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasCode() => $_has(3);
  @$pb.TagNumber(4)
  void clearCode() => $_clearField(4);

  /// The `state` `Begin` returned, echoed by the IdP's redirect. Single use.
  @$pb.TagNumber(5)
  $core.String get state => $_getSZ(4);
  @$pb.TagNumber(5)
  set state($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasState() => $_has(4);
  @$pb.TagNumber(5)
  void clearState() => $_clearField(5);

  /// The PKCE verifier whose S256 challenge went to `Begin` (RFC 7636: 43-128
  /// unreserved characters).
  @$pb.TagNumber(6)
  $core.String get codeVerifier => $_getSZ(5);
  @$pb.TagNumber(6)
  set codeVerifier($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasCodeVerifier() => $_has(5);
  @$pb.TagNumber(6)
  void clearCodeVerifier() => $_clearField(6);
}

class ExchangeResponse extends $pb.GeneratedMessage {
  factory ExchangeResponse({
    $core.String? accessToken,
    $core.String? tokenType,
    $fixnum.Int64? expiresAt,
    WhoAmIResponse? principal,
    $core.String? refreshHandle,
    $fixnum.Int64? sessionExpiresAt,
  }) {
    final result = create();
    if (accessToken != null) result.accessToken = accessToken;
    if (tokenType != null) result.tokenType = tokenType;
    if (expiresAt != null) result.expiresAt = expiresAt;
    if (principal != null) result.principal = principal;
    if (refreshHandle != null) result.refreshHandle = refreshHandle;
    if (sessionExpiresAt != null) result.sessionExpiresAt = sessionExpiresAt;
    return result;
  }

  ExchangeResponse._();

  factory ExchangeResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ExchangeResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ExchangeResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'accessToken')
    ..aOS(2, _omitFieldNames ? '' : 'tokenType')
    ..a<$fixnum.Int64>(
        3, _omitFieldNames ? '' : 'expiresAt', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOM<WhoAmIResponse>(4, _omitFieldNames ? '' : 'principal',
        subBuilder: WhoAmIResponse.create)
    ..aOS(5, _omitFieldNames ? '' : 'refreshHandle')
    ..a<$fixnum.Int64>(
        6, _omitFieldNames ? '' : 'sessionExpiresAt', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ExchangeResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ExchangeResponse copyWith(void Function(ExchangeResponse) updates) =>
      super.copyWith((message) => updates(message as ExchangeResponse))
          as ExchangeResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ExchangeResponse create() => ExchangeResponse._();
  @$core.override
  ExchangeResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ExchangeResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ExchangeResponse>(create);
  static ExchangeResponse? _defaultInstance;

  /// The agent token (compact JWT); send it as `authorization: Bearer <token>`.
  @$pb.TagNumber(1)
  $core.String get accessToken => $_getSZ(0);
  @$pb.TagNumber(1)
  set accessToken($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasAccessToken() => $_has(0);
  @$pb.TagNumber(1)
  void clearAccessToken() => $_clearField(1);

  /// Always `Bearer`.
  @$pb.TagNumber(2)
  $core.String get tokenType => $_getSZ(1);
  @$pb.TagNumber(2)
  set tokenType($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasTokenType() => $_has(1);
  @$pb.TagNumber(2)
  void clearTokenType() => $_clearField(2);

  /// Unix seconds after which the token is rejected.
  @$pb.TagNumber(3)
  $fixnum.Int64 get expiresAt => $_getI64(2);
  @$pb.TagNumber(3)
  set expiresAt($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasExpiresAt() => $_has(2);
  @$pb.TagNumber(3)
  void clearExpiresAt() => $_clearField(3);

  /// The identity the token carries (the same fields `WhoAmI` returns).
  @$pb.TagNumber(4)
  WhoAmIResponse get principal => $_getN(3);
  @$pb.TagNumber(4)
  set principal(WhoAmIResponse value) => $_setField(4, value);
  @$pb.TagNumber(4)
  $core.bool hasPrincipal() => $_has(3);
  @$pb.TagNumber(4)
  void clearPrincipal() => $_clearField(4);
  @$pb.TagNumber(4)
  WhoAmIResponse ensurePrincipal() => $_ensure(3);

  /// Opaque; send it to `Refresh` before `expires_at`. Each use returns a new one.
  @$pb.TagNumber(5)
  $core.String get refreshHandle => $_getSZ(4);
  @$pb.TagNumber(5)
  set refreshHandle($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasRefreshHandle() => $_has(4);
  @$pb.TagNumber(5)
  void clearRefreshHandle() => $_clearField(5);

  /// Unix seconds after which the session cannot be refreshed; sign in again.
  @$pb.TagNumber(6)
  $fixnum.Int64 get sessionExpiresAt => $_getI64(5);
  @$pb.TagNumber(6)
  set sessionExpiresAt($fixnum.Int64 value) => $_setInt64(5, value);
  @$pb.TagNumber(6)
  $core.bool hasSessionExpiresAt() => $_has(5);
  @$pb.TagNumber(6)
  void clearSessionExpiresAt() => $_clearField(6);
}

class IssuersRequest extends $pb.GeneratedMessage {
  factory IssuersRequest() => create();

  IssuersRequest._();

  factory IssuersRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory IssuersRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'IssuersRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  IssuersRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  IssuersRequest copyWith(void Function(IssuersRequest) updates) =>
      super.copyWith((message) => updates(message as IssuersRequest))
          as IssuersRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static IssuersRequest create() => IssuersRequest._();
  @$core.override
  IssuersRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static IssuersRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<IssuersRequest>(create);
  static IssuersRequest? _defaultInstance;
}

class IssuersResponse extends $pb.GeneratedMessage {
  factory IssuersResponse({
    $core.Iterable<LoginIssuer>? issuers,
  }) {
    final result = create();
    if (issuers != null) result.issuers.addAll(issuers);
    return result;
  }

  IssuersResponse._();

  factory IssuersResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory IssuersResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'IssuersResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<LoginIssuer>(1, _omitFieldNames ? '' : 'issuers',
        subBuilder: LoginIssuer.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  IssuersResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  IssuersResponse copyWith(void Function(IssuersResponse) updates) =>
      super.copyWith((message) => updates(message as IssuersResponse))
          as IssuersResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static IssuersResponse create() => IssuersResponse._();
  @$core.override
  IssuersResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static IssuersResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<IssuersResponse>(create);
  static IssuersResponse? _defaultInstance;

  /// Empty when browser sign-in is not configured (no `[auth] redirect_uris`).
  @$pb.TagNumber(1)
  $pb.PbList<LoginIssuer> get issuers => $_getList(0);
}

class LoginIssuer extends $pb.GeneratedMessage {
  factory LoginIssuer({
    $core.String? name,
    $core.String? profile,
  }) {
    final result = create();
    if (name != null) result.name = name;
    if (profile != null) result.profile = profile;
    return result;
  }

  LoginIssuer._();

  factory LoginIssuer.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LoginIssuer.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LoginIssuer',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'name')
    ..aOS(2, _omitFieldNames ? '' : 'profile')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LoginIssuer clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LoginIssuer copyWith(void Function(LoginIssuer) updates) =>
      super.copyWith((message) => updates(message as LoginIssuer))
          as LoginIssuer;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LoginIssuer create() => LoginIssuer._();
  @$core.override
  LoginIssuer createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LoginIssuer getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LoginIssuer>(create);
  static LoginIssuer? _defaultInstance;

  /// The `[[auth.issuers]]` name `Begin` takes.
  @$pb.TagNumber(1)
  $core.String get name => $_getSZ(0);
  @$pb.TagNumber(1)
  set name($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasName() => $_has(0);
  @$pb.TagNumber(1)
  void clearName() => $_clearField(1);

  /// `google` | `entra` | `generic`, so a client can label the button.
  @$pb.TagNumber(2)
  $core.String get profile => $_getSZ(1);
  @$pb.TagNumber(2)
  set profile($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasProfile() => $_has(1);
  @$pb.TagNumber(2)
  void clearProfile() => $_clearField(2);
}

class BeginRequest extends $pb.GeneratedMessage {
  factory BeginRequest({
    $core.String? issuer,
    $core.String? redirectUri,
    $core.String? codeChallenge,
  }) {
    final result = create();
    if (issuer != null) result.issuer = issuer;
    if (redirectUri != null) result.redirectUri = redirectUri;
    if (codeChallenge != null) result.codeChallenge = codeChallenge;
    return result;
  }

  BeginRequest._();

  factory BeginRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory BeginRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'BeginRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'issuer')
    ..aOS(2, _omitFieldNames ? '' : 'redirectUri')
    ..aOS(3, _omitFieldNames ? '' : 'codeChallenge')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  BeginRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  BeginRequest copyWith(void Function(BeginRequest) updates) =>
      super.copyWith((message) => updates(message as BeginRequest))
          as BeginRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static BeginRequest create() => BeginRequest._();
  @$core.override
  BeginRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static BeginRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<BeginRequest>(create);
  static BeginRequest? _defaultInstance;

  /// A name from `Issuers`.
  @$pb.TagNumber(1)
  $core.String get issuer => $_getSZ(0);
  @$pb.TagNumber(1)
  set issuer($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasIssuer() => $_has(0);
  @$pb.TagNumber(1)
  void clearIssuer() => $_clearField(1);

  /// Where the IdP sends the browser back; exactly one of `[auth] redirect_uris`.
  @$pb.TagNumber(2)
  $core.String get redirectUri => $_getSZ(1);
  @$pb.TagNumber(2)
  set redirectUri($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasRedirectUri() => $_has(1);
  @$pb.TagNumber(2)
  void clearRedirectUri() => $_clearField(2);

  /// The PKCE S256 challenge: base64url (no padding) of SHA-256 of the verifier,
  /// 43 characters. The verifier stays with the client until `Exchange`.
  @$pb.TagNumber(3)
  $core.String get codeChallenge => $_getSZ(2);
  @$pb.TagNumber(3)
  set codeChallenge($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasCodeChallenge() => $_has(2);
  @$pb.TagNumber(3)
  void clearCodeChallenge() => $_clearField(3);
}

class BeginResponse extends $pb.GeneratedMessage {
  factory BeginResponse({
    $core.String? authorizeUrl,
    $core.String? state,
    $fixnum.Int64? expiresAt,
  }) {
    final result = create();
    if (authorizeUrl != null) result.authorizeUrl = authorizeUrl;
    if (state != null) result.state = state;
    if (expiresAt != null) result.expiresAt = expiresAt;
    return result;
  }

  BeginResponse._();

  factory BeginResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory BeginResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'BeginResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'authorizeUrl')
    ..aOS(2, _omitFieldNames ? '' : 'state')
    ..a<$fixnum.Int64>(
        3, _omitFieldNames ? '' : 'expiresAt', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  BeginResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  BeginResponse copyWith(void Function(BeginResponse) updates) =>
      super.copyWith((message) => updates(message as BeginResponse))
          as BeginResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static BeginResponse create() => BeginResponse._();
  @$core.override
  BeginResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static BeginResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<BeginResponse>(create);
  static BeginResponse? _defaultInstance;

  /// The IdP's authorization URL, with every parameter set; navigate to it.
  @$pb.TagNumber(1)
  $core.String get authorizeUrl => $_getSZ(0);
  @$pb.TagNumber(1)
  set authorizeUrl($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasAuthorizeUrl() => $_has(0);
  @$pb.TagNumber(1)
  void clearAuthorizeUrl() => $_clearField(1);

  /// Opaque and single use; the IdP echoes it to the redirect URI.
  @$pb.TagNumber(2)
  $core.String get state => $_getSZ(1);
  @$pb.TagNumber(2)
  set state($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasState() => $_has(1);
  @$pb.TagNumber(2)
  void clearState() => $_clearField(2);

  /// Unix seconds after which `state` is no longer accepted.
  @$pb.TagNumber(3)
  $fixnum.Int64 get expiresAt => $_getI64(2);
  @$pb.TagNumber(3)
  set expiresAt($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasExpiresAt() => $_has(2);
  @$pb.TagNumber(3)
  void clearExpiresAt() => $_clearField(3);
}

class JwksRequest extends $pb.GeneratedMessage {
  factory JwksRequest() => create();

  JwksRequest._();

  factory JwksRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory JwksRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'JwksRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  JwksRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  JwksRequest copyWith(void Function(JwksRequest) updates) =>
      super.copyWith((message) => updates(message as JwksRequest))
          as JwksRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static JwksRequest create() => JwksRequest._();
  @$core.override
  JwksRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static JwksRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<JwksRequest>(create);
  static JwksRequest? _defaultInstance;
}

class JwksResponse extends $pb.GeneratedMessage {
  factory JwksResponse({
    $core.String? jwksJson,
  }) {
    final result = create();
    if (jwksJson != null) result.jwksJson = jwksJson;
    return result;
  }

  JwksResponse._();

  factory JwksResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory JwksResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'JwksResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'jwksJson')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  JwksResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  JwksResponse copyWith(void Function(JwksResponse) updates) =>
      super.copyWith((message) => updates(message as JwksResponse))
          as JwksResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static JwksResponse create() => JwksResponse._();
  @$core.override
  JwksResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static JwksResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<JwksResponse>(create);
  static JwksResponse? _defaultInstance;

  /// The JWK Set document (`{"keys":[...]}`), as served at `/.well-known/jwks.json`.
  @$pb.TagNumber(1)
  $core.String get jwksJson => $_getSZ(0);
  @$pb.TagNumber(1)
  set jwksJson($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasJwksJson() => $_has(0);
  @$pb.TagNumber(1)
  void clearJwksJson() => $_clearField(1);
}

class WhoAmIRequest extends $pb.GeneratedMessage {
  factory WhoAmIRequest() => create();

  WhoAmIRequest._();

  factory WhoAmIRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory WhoAmIRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'WhoAmIRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WhoAmIRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WhoAmIRequest copyWith(void Function(WhoAmIRequest) updates) =>
      super.copyWith((message) => updates(message as WhoAmIRequest))
          as WhoAmIRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static WhoAmIRequest create() => WhoAmIRequest._();
  @$core.override
  WhoAmIRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static WhoAmIRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<WhoAmIRequest>(create);
  static WhoAmIRequest? _defaultInstance;
}

class WhoAmIResponse extends $pb.GeneratedMessage {
  factory WhoAmIResponse({
    $core.String? tenant,
    $core.String? subject,
    $core.String? issuer,
    $core.String? email,
    $core.Iterable<$core.String>? roles,
    $core.Iterable<$core.String>? permissions,
    $core.bool? permsRef,
    $core.Iterable<$core.String>? amr,
    $fixnum.Int64? expiresAt,
    $core.String? sid,
  }) {
    final result = create();
    if (tenant != null) result.tenant = tenant;
    if (subject != null) result.subject = subject;
    if (issuer != null) result.issuer = issuer;
    if (email != null) result.email = email;
    if (roles != null) result.roles.addAll(roles);
    if (permissions != null) result.permissions.addAll(permissions);
    if (permsRef != null) result.permsRef = permsRef;
    if (amr != null) result.amr.addAll(amr);
    if (expiresAt != null) result.expiresAt = expiresAt;
    if (sid != null) result.sid = sid;
    return result;
  }

  WhoAmIResponse._();

  factory WhoAmIResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory WhoAmIResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'WhoAmIResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'tenant')
    ..aOS(2, _omitFieldNames ? '' : 'subject')
    ..aOS(3, _omitFieldNames ? '' : 'issuer')
    ..aOS(4, _omitFieldNames ? '' : 'email')
    ..pPS(5, _omitFieldNames ? '' : 'roles')
    ..pPS(6, _omitFieldNames ? '' : 'permissions')
    ..aOB(7, _omitFieldNames ? '' : 'permsRef')
    ..pPS(8, _omitFieldNames ? '' : 'amr')
    ..a<$fixnum.Int64>(
        9, _omitFieldNames ? '' : 'expiresAt', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOS(10, _omitFieldNames ? '' : 'sid')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WhoAmIResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  WhoAmIResponse copyWith(void Function(WhoAmIResponse) updates) =>
      super.copyWith((message) => updates(message as WhoAmIResponse))
          as WhoAmIResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static WhoAmIResponse create() => WhoAmIResponse._();
  @$core.override
  WhoAmIResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static WhoAmIResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<WhoAmIResponse>(create);
  static WhoAmIResponse? _defaultInstance;

  /// The verified organization; the caller's `x-agent-user-id`.
  @$pb.TagNumber(1)
  $core.String get tenant => $_getSZ(0);
  @$pb.TagNumber(1)
  set tenant($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasTenant() => $_has(0);
  @$pb.TagNumber(1)
  void clearTenant() => $_clearField(1);

  /// `user:<issuer>/<sub>` for a person, `svc:<name>` for a service.
  @$pb.TagNumber(2)
  $core.String get subject => $_getSZ(1);
  @$pb.TagNumber(2)
  set subject($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasSubject() => $_has(1);
  @$pb.TagNumber(2)
  void clearSubject() => $_clearField(2);

  /// The login issuer's configured name.
  @$pb.TagNumber(3)
  $core.String get issuer => $_getSZ(2);
  @$pb.TagNumber(3)
  set issuer($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasIssuer() => $_has(2);
  @$pb.TagNumber(3)
  void clearIssuer() => $_clearField(3);

  /// Empty when the identity provider sent none.
  @$pb.TagNumber(4)
  $core.String get email => $_getSZ(3);
  @$pb.TagNumber(4)
  set email($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasEmail() => $_has(3);
  @$pb.TagNumber(4)
  void clearEmail() => $_clearField(4);

  @$pb.TagNumber(5)
  $pb.PbList<$core.String> get roles => $_getList(4);

  /// `action:resource` pairs the roles grant in the caller's own tenant, as of when
  /// the token was issued. Empty with `perms_ref` set when the list is too large to
  /// embed; the roles are authoritative either way.
  @$pb.TagNumber(6)
  $pb.PbList<$core.String> get permissions => $_getList(5);

  @$pb.TagNumber(7)
  $core.bool get permsRef => $_getBF(6);
  @$pb.TagNumber(7)
  set permsRef($core.bool value) => $_setBool(6, value);
  @$pb.TagNumber(7)
  $core.bool hasPermsRef() => $_has(6);
  @$pb.TagNumber(7)
  void clearPermsRef() => $_clearField(7);

  /// How the caller authenticated (`oidc:<issuer>`).
  @$pb.TagNumber(8)
  $pb.PbList<$core.String> get amr => $_getList(7);

  /// Unix seconds after which the token is rejected.
  @$pb.TagNumber(9)
  $fixnum.Int64 get expiresAt => $_getI64(8);
  @$pb.TagNumber(9)
  set expiresAt($fixnum.Int64 value) => $_setInt64(8, value);
  @$pb.TagNumber(9)
  $core.bool hasExpiresAt() => $_has(8);
  @$pb.TagNumber(9)
  void clearExpiresAt() => $_clearField(9);

  /// The auth session the token belongs to.
  @$pb.TagNumber(10)
  $core.String get sid => $_getSZ(9);
  @$pb.TagNumber(10)
  set sid($core.String value) => $_setString(9, value);
  @$pb.TagNumber(10)
  $core.bool hasSid() => $_has(9);
  @$pb.TagNumber(10)
  void clearSid() => $_clearField(10);
}

class RefreshRequest extends $pb.GeneratedMessage {
  factory RefreshRequest({
    $core.String? refreshHandle,
  }) {
    final result = create();
    if (refreshHandle != null) result.refreshHandle = refreshHandle;
    return result;
  }

  RefreshRequest._();

  factory RefreshRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RefreshRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RefreshRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'refreshHandle')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RefreshRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RefreshRequest copyWith(void Function(RefreshRequest) updates) =>
      super.copyWith((message) => updates(message as RefreshRequest))
          as RefreshRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RefreshRequest create() => RefreshRequest._();
  @$core.override
  RefreshRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RefreshRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RefreshRequest>(create);
  static RefreshRequest? _defaultInstance;

  /// The handle from the last `Exchange` or `Refresh`. At most 1 KiB.
  @$pb.TagNumber(1)
  $core.String get refreshHandle => $_getSZ(0);
  @$pb.TagNumber(1)
  set refreshHandle($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRefreshHandle() => $_has(0);
  @$pb.TagNumber(1)
  void clearRefreshHandle() => $_clearField(1);
}

class LogoutRequest extends $pb.GeneratedMessage {
  factory LogoutRequest() => create();

  LogoutRequest._();

  factory LogoutRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LogoutRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LogoutRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LogoutRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LogoutRequest copyWith(void Function(LogoutRequest) updates) =>
      super.copyWith((message) => updates(message as LogoutRequest))
          as LogoutRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LogoutRequest create() => LogoutRequest._();
  @$core.override
  LogoutRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LogoutRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LogoutRequest>(create);
  static LogoutRequest? _defaultInstance;
}

class LogoutResponse extends $pb.GeneratedMessage {
  factory LogoutResponse({
    $core.bool? revoked,
  }) {
    final result = create();
    if (revoked != null) result.revoked = revoked;
    return result;
  }

  LogoutResponse._();

  factory LogoutResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory LogoutResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'LogoutResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'revoked')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LogoutResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  LogoutResponse copyWith(void Function(LogoutResponse) updates) =>
      super.copyWith((message) => updates(message as LogoutResponse))
          as LogoutResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static LogoutResponse create() => LogoutResponse._();
  @$core.override
  LogoutResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static LogoutResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<LogoutResponse>(create);
  static LogoutResponse? _defaultInstance;

  /// False when the session was already revoked or expired.
  @$pb.TagNumber(1)
  $core.bool get revoked => $_getBF(0);
  @$pb.TagNumber(1)
  set revoked($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRevoked() => $_has(0);
  @$pb.TagNumber(1)
  void clearRevoked() => $_clearField(1);
}

class ListMySessionsRequest extends $pb.GeneratedMessage {
  factory ListMySessionsRequest() => create();

  ListMySessionsRequest._();

  factory ListMySessionsRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ListMySessionsRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ListMySessionsRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListMySessionsRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListMySessionsRequest copyWith(
          void Function(ListMySessionsRequest) updates) =>
      super.copyWith((message) => updates(message as ListMySessionsRequest))
          as ListMySessionsRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ListMySessionsRequest create() => ListMySessionsRequest._();
  @$core.override
  ListMySessionsRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ListMySessionsRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ListMySessionsRequest>(create);
  static ListMySessionsRequest? _defaultInstance;
}

class ListSessionsRequest extends $pb.GeneratedMessage {
  factory ListSessionsRequest({
    $core.String? tenant,
  }) {
    final result = create();
    if (tenant != null) result.tenant = tenant;
    return result;
  }

  ListSessionsRequest._();

  factory ListSessionsRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ListSessionsRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ListSessionsRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'tenant')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListSessionsRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListSessionsRequest copyWith(void Function(ListSessionsRequest) updates) =>
      super.copyWith((message) => updates(message as ListSessionsRequest))
          as ListSessionsRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ListSessionsRequest create() => ListSessionsRequest._();
  @$core.override
  ListSessionsRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ListSessionsRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ListSessionsRequest>(create);
  static ListSessionsRequest? _defaultInstance;

  /// Empty ⇒ the caller's own tenant. Another tenant needs a host-global role.
  @$pb.TagNumber(1)
  $core.String get tenant => $_getSZ(0);
  @$pb.TagNumber(1)
  set tenant($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasTenant() => $_has(0);
  @$pb.TagNumber(1)
  void clearTenant() => $_clearField(1);
}

class ListSessionsResponse extends $pb.GeneratedMessage {
  factory ListSessionsResponse({
    $core.Iterable<AuthSessionInfo>? sessions,
  }) {
    final result = create();
    if (sessions != null) result.sessions.addAll(sessions);
    return result;
  }

  ListSessionsResponse._();

  factory ListSessionsResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ListSessionsResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ListSessionsResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<AuthSessionInfo>(1, _omitFieldNames ? '' : 'sessions',
        subBuilder: AuthSessionInfo.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListSessionsResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListSessionsResponse copyWith(void Function(ListSessionsResponse) updates) =>
      super.copyWith((message) => updates(message as ListSessionsResponse))
          as ListSessionsResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ListSessionsResponse create() => ListSessionsResponse._();
  @$core.override
  ListSessionsResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ListSessionsResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ListSessionsResponse>(create);
  static ListSessionsResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<AuthSessionInfo> get sessions => $_getList(0);
}

class RevokeMySessionRequest extends $pb.GeneratedMessage {
  factory RevokeMySessionRequest({
    $core.String? sid,
  }) {
    final result = create();
    if (sid != null) result.sid = sid;
    return result;
  }

  RevokeMySessionRequest._();

  factory RevokeMySessionRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RevokeMySessionRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RevokeMySessionRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'sid')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RevokeMySessionRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RevokeMySessionRequest copyWith(
          void Function(RevokeMySessionRequest) updates) =>
      super.copyWith((message) => updates(message as RevokeMySessionRequest))
          as RevokeMySessionRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RevokeMySessionRequest create() => RevokeMySessionRequest._();
  @$core.override
  RevokeMySessionRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RevokeMySessionRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RevokeMySessionRequest>(create);
  static RevokeMySessionRequest? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get sid => $_getSZ(0);
  @$pb.TagNumber(1)
  set sid($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasSid() => $_has(0);
  @$pb.TagNumber(1)
  void clearSid() => $_clearField(1);
}

class RevokeSessionRequest extends $pb.GeneratedMessage {
  factory RevokeSessionRequest({
    $core.String? tenant,
    $core.String? sid,
  }) {
    final result = create();
    if (tenant != null) result.tenant = tenant;
    if (sid != null) result.sid = sid;
    return result;
  }

  RevokeSessionRequest._();

  factory RevokeSessionRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RevokeSessionRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RevokeSessionRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'tenant')
    ..aOS(2, _omitFieldNames ? '' : 'sid')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RevokeSessionRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RevokeSessionRequest copyWith(void Function(RevokeSessionRequest) updates) =>
      super.copyWith((message) => updates(message as RevokeSessionRequest))
          as RevokeSessionRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RevokeSessionRequest create() => RevokeSessionRequest._();
  @$core.override
  RevokeSessionRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RevokeSessionRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RevokeSessionRequest>(create);
  static RevokeSessionRequest? _defaultInstance;

  /// Empty ⇒ the caller's own tenant.
  @$pb.TagNumber(1)
  $core.String get tenant => $_getSZ(0);
  @$pb.TagNumber(1)
  set tenant($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasTenant() => $_has(0);
  @$pb.TagNumber(1)
  void clearTenant() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get sid => $_getSZ(1);
  @$pb.TagNumber(2)
  set sid($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasSid() => $_has(1);
  @$pb.TagNumber(2)
  void clearSid() => $_clearField(2);
}

class RevokeSessionResponse extends $pb.GeneratedMessage {
  factory RevokeSessionResponse({
    $core.bool? revoked,
  }) {
    final result = create();
    if (revoked != null) result.revoked = revoked;
    return result;
  }

  RevokeSessionResponse._();

  factory RevokeSessionResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RevokeSessionResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RevokeSessionResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'revoked')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RevokeSessionResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RevokeSessionResponse copyWith(
          void Function(RevokeSessionResponse) updates) =>
      super.copyWith((message) => updates(message as RevokeSessionResponse))
          as RevokeSessionResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RevokeSessionResponse create() => RevokeSessionResponse._();
  @$core.override
  RevokeSessionResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RevokeSessionResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RevokeSessionResponse>(create);
  static RevokeSessionResponse? _defaultInstance;

  /// False when there was no live session with that id.
  @$pb.TagNumber(1)
  $core.bool get revoked => $_getBF(0);
  @$pb.TagNumber(1)
  set revoked($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasRevoked() => $_has(0);
  @$pb.TagNumber(1)
  void clearRevoked() => $_clearField(1);
}

/// One auth session. Never carries a token or a refresh handle.
class AuthSessionInfo extends $pb.GeneratedMessage {
  factory AuthSessionInfo({
    $core.String? sid,
    $core.String? tenant,
    $core.String? subject,
    $core.String? issuer,
    $core.String? email,
    $core.String? clientKind,
    $fixnum.Int64? createdAt,
    $fixnum.Int64? lastSeenAt,
    $fixnum.Int64? expiresAt,
    $fixnum.Int64? revokedAt,
    $core.String? revokeReason,
    $core.bool? current,
  }) {
    final result = create();
    if (sid != null) result.sid = sid;
    if (tenant != null) result.tenant = tenant;
    if (subject != null) result.subject = subject;
    if (issuer != null) result.issuer = issuer;
    if (email != null) result.email = email;
    if (clientKind != null) result.clientKind = clientKind;
    if (createdAt != null) result.createdAt = createdAt;
    if (lastSeenAt != null) result.lastSeenAt = lastSeenAt;
    if (expiresAt != null) result.expiresAt = expiresAt;
    if (revokedAt != null) result.revokedAt = revokedAt;
    if (revokeReason != null) result.revokeReason = revokeReason;
    if (current != null) result.current = current;
    return result;
  }

  AuthSessionInfo._();

  factory AuthSessionInfo.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory AuthSessionInfo.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'AuthSessionInfo',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'sid')
    ..aOS(2, _omitFieldNames ? '' : 'tenant')
    ..aOS(3, _omitFieldNames ? '' : 'subject')
    ..aOS(4, _omitFieldNames ? '' : 'issuer')
    ..aOS(5, _omitFieldNames ? '' : 'email')
    ..aOS(6, _omitFieldNames ? '' : 'clientKind')
    ..a<$fixnum.Int64>(
        7, _omitFieldNames ? '' : 'createdAt', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..a<$fixnum.Int64>(
        8, _omitFieldNames ? '' : 'lastSeenAt', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..a<$fixnum.Int64>(
        9, _omitFieldNames ? '' : 'expiresAt', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..a<$fixnum.Int64>(
        10, _omitFieldNames ? '' : 'revokedAt', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..aOS(11, _omitFieldNames ? '' : 'revokeReason')
    ..aOB(12, _omitFieldNames ? '' : 'current')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AuthSessionInfo clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  AuthSessionInfo copyWith(void Function(AuthSessionInfo) updates) =>
      super.copyWith((message) => updates(message as AuthSessionInfo))
          as AuthSessionInfo;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static AuthSessionInfo create() => AuthSessionInfo._();
  @$core.override
  AuthSessionInfo createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static AuthSessionInfo getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<AuthSessionInfo>(create);
  static AuthSessionInfo? _defaultInstance;

  @$pb.TagNumber(1)
  $core.String get sid => $_getSZ(0);
  @$pb.TagNumber(1)
  set sid($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasSid() => $_has(0);
  @$pb.TagNumber(1)
  void clearSid() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get tenant => $_getSZ(1);
  @$pb.TagNumber(2)
  set tenant($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasTenant() => $_has(1);
  @$pb.TagNumber(2)
  void clearTenant() => $_clearField(2);

  @$pb.TagNumber(3)
  $core.String get subject => $_getSZ(2);
  @$pb.TagNumber(3)
  set subject($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasSubject() => $_has(2);
  @$pb.TagNumber(3)
  void clearSubject() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get issuer => $_getSZ(3);
  @$pb.TagNumber(4)
  set issuer($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasIssuer() => $_has(3);
  @$pb.TagNumber(4)
  void clearIssuer() => $_clearField(4);

  @$pb.TagNumber(5)
  $core.String get email => $_getSZ(4);
  @$pb.TagNumber(5)
  set email($core.String value) => $_setString(4, value);
  @$pb.TagNumber(5)
  $core.bool hasEmail() => $_has(4);
  @$pb.TagNumber(5)
  void clearEmail() => $_clearField(5);

  /// `portal` | `cli` | `service` | `unspecified`.
  @$pb.TagNumber(6)
  $core.String get clientKind => $_getSZ(5);
  @$pb.TagNumber(6)
  set clientKind($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasClientKind() => $_has(5);
  @$pb.TagNumber(6)
  void clearClientKind() => $_clearField(6);

  /// Unix seconds.
  @$pb.TagNumber(7)
  $fixnum.Int64 get createdAt => $_getI64(6);
  @$pb.TagNumber(7)
  set createdAt($fixnum.Int64 value) => $_setInt64(6, value);
  @$pb.TagNumber(7)
  $core.bool hasCreatedAt() => $_has(6);
  @$pb.TagNumber(7)
  void clearCreatedAt() => $_clearField(7);

  @$pb.TagNumber(8)
  $fixnum.Int64 get lastSeenAt => $_getI64(7);
  @$pb.TagNumber(8)
  set lastSeenAt($fixnum.Int64 value) => $_setInt64(7, value);
  @$pb.TagNumber(8)
  $core.bool hasLastSeenAt() => $_has(7);
  @$pb.TagNumber(8)
  void clearLastSeenAt() => $_clearField(8);

  @$pb.TagNumber(9)
  $fixnum.Int64 get expiresAt => $_getI64(8);
  @$pb.TagNumber(9)
  set expiresAt($fixnum.Int64 value) => $_setInt64(8, value);
  @$pb.TagNumber(9)
  $core.bool hasExpiresAt() => $_has(8);
  @$pb.TagNumber(9)
  void clearExpiresAt() => $_clearField(9);

  /// Zero while live.
  @$pb.TagNumber(10)
  $fixnum.Int64 get revokedAt => $_getI64(9);
  @$pb.TagNumber(10)
  set revokedAt($fixnum.Int64 value) => $_setInt64(9, value);
  @$pb.TagNumber(10)
  $core.bool hasRevokedAt() => $_has(9);
  @$pb.TagNumber(10)
  void clearRevokedAt() => $_clearField(10);

  /// `logout` | `operator` | `reuse` | `binding` | empty.
  @$pb.TagNumber(11)
  $core.String get revokeReason => $_getSZ(10);
  @$pb.TagNumber(11)
  set revokeReason($core.String value) => $_setString(10, value);
  @$pb.TagNumber(11)
  $core.bool hasRevokeReason() => $_has(10);
  @$pb.TagNumber(11)
  void clearRevokeReason() => $_clearField(11);

  /// The session the caller's own token names.
  @$pb.TagNumber(12)
  $core.bool get current => $_getBF(11);
  @$pb.TagNumber(12)
  set current($core.bool value) => $_setBool(11, value);
  @$pb.TagNumber(12)
  $core.bool hasCurrent() => $_has(11);
  @$pb.TagNumber(12)
  void clearCurrent() => $_clearField(12);
}

/// Grants roles to a subject in one tenant. Roles are resolved when a token is
/// minted (sign-in and every refresh): the union of the tenant's bindings that
/// name the caller.
class RoleBinding extends $pb.GeneratedMessage {
  factory RoleBinding({
    $core.String? id,
    $core.String? tenant,
    $core.String? subjectKind,
    $core.String? subject,
    $core.Iterable<$core.String>? roles,
    $core.String? grantedBy,
    $fixnum.Int64? grantedAt,
    $fixnum.Int64? expiresAt,
  }) {
    final result = create();
    if (id != null) result.id = id;
    if (tenant != null) result.tenant = tenant;
    if (subjectKind != null) result.subjectKind = subjectKind;
    if (subject != null) result.subject = subject;
    if (roles != null) result.roles.addAll(roles);
    if (grantedBy != null) result.grantedBy = grantedBy;
    if (grantedAt != null) result.grantedAt = grantedAt;
    if (expiresAt != null) result.expiresAt = expiresAt;
    return result;
  }

  RoleBinding._();

  factory RoleBinding.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RoleBinding.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RoleBinding',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'id')
    ..aOS(2, _omitFieldNames ? '' : 'tenant')
    ..aOS(3, _omitFieldNames ? '' : 'subjectKind')
    ..aOS(4, _omitFieldNames ? '' : 'subject')
    ..pPS(5, _omitFieldNames ? '' : 'roles')
    ..aOS(6, _omitFieldNames ? '' : 'grantedBy')
    ..a<$fixnum.Int64>(
        7, _omitFieldNames ? '' : 'grantedAt', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..a<$fixnum.Int64>(
        8, _omitFieldNames ? '' : 'expiresAt', $pb.PbFieldType.OU6,
        defaultOrMaker: $fixnum.Int64.ZERO)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RoleBinding clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RoleBinding copyWith(void Function(RoleBinding) updates) =>
      super.copyWith((message) => updates(message as RoleBinding))
          as RoleBinding;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RoleBinding create() => RoleBinding._();
  @$core.override
  RoleBinding createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RoleBinding getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RoleBinding>(create);
  static RoleBinding? _defaultInstance;

  /// Path-safe id, unique in the tenant.
  @$pb.TagNumber(1)
  $core.String get id => $_getSZ(0);
  @$pb.TagNumber(1)
  set id($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasId() => $_has(0);
  @$pb.TagNumber(1)
  void clearId() => $_clearField(1);

  /// Empty on write ⇒ the caller's own tenant. Another tenant needs a host-global
  /// role.
  @$pb.TagNumber(2)
  $core.String get tenant => $_getSZ(1);
  @$pb.TagNumber(2)
  set tenant($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasTenant() => $_has(1);
  @$pb.TagNumber(2)
  void clearTenant() => $_clearField(2);

  /// `sub` (subject `<issuer>/<sub>`), `email` (a verified address), `domain`
  /// (every verified address in it) or `mtls_san` (a service peer).
  @$pb.TagNumber(3)
  $core.String get subjectKind => $_getSZ(2);
  @$pb.TagNumber(3)
  set subjectKind($core.String value) => $_setString(2, value);
  @$pb.TagNumber(3)
  $core.bool hasSubjectKind() => $_has(2);
  @$pb.TagNumber(3)
  void clearSubjectKind() => $_clearField(3);

  @$pb.TagNumber(4)
  $core.String get subject => $_getSZ(3);
  @$pb.TagNumber(4)
  set subject($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasSubject() => $_has(3);
  @$pb.TagNumber(4)
  void clearSubject() => $_clearField(4);

  /// Role names; each must exist when the binding is written.
  @$pb.TagNumber(5)
  $pb.PbList<$core.String> get roles => $_getList(4);

  /// Set by the server: who wrote the binding, and when (Unix seconds).
  @$pb.TagNumber(6)
  $core.String get grantedBy => $_getSZ(5);
  @$pb.TagNumber(6)
  set grantedBy($core.String value) => $_setString(5, value);
  @$pb.TagNumber(6)
  $core.bool hasGrantedBy() => $_has(5);
  @$pb.TagNumber(6)
  void clearGrantedBy() => $_clearField(6);

  @$pb.TagNumber(7)
  $fixnum.Int64 get grantedAt => $_getI64(6);
  @$pb.TagNumber(7)
  set grantedAt($fixnum.Int64 value) => $_setInt64(6, value);
  @$pb.TagNumber(7)
  $core.bool hasGrantedAt() => $_has(6);
  @$pb.TagNumber(7)
  void clearGrantedAt() => $_clearField(7);

  /// Unix seconds; zero ⇒ never expires.
  @$pb.TagNumber(8)
  $fixnum.Int64 get expiresAt => $_getI64(7);
  @$pb.TagNumber(8)
  set expiresAt($fixnum.Int64 value) => $_setInt64(7, value);
  @$pb.TagNumber(8)
  $core.bool hasExpiresAt() => $_has(7);
  @$pb.TagNumber(8)
  void clearExpiresAt() => $_clearField(8);
}

class ListBindingsRequest extends $pb.GeneratedMessage {
  factory ListBindingsRequest({
    $core.String? tenant,
  }) {
    final result = create();
    if (tenant != null) result.tenant = tenant;
    return result;
  }

  ListBindingsRequest._();

  factory ListBindingsRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ListBindingsRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ListBindingsRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'tenant')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListBindingsRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListBindingsRequest copyWith(void Function(ListBindingsRequest) updates) =>
      super.copyWith((message) => updates(message as ListBindingsRequest))
          as ListBindingsRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ListBindingsRequest create() => ListBindingsRequest._();
  @$core.override
  ListBindingsRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ListBindingsRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ListBindingsRequest>(create);
  static ListBindingsRequest? _defaultInstance;

  /// Empty ⇒ the caller's own tenant.
  @$pb.TagNumber(1)
  $core.String get tenant => $_getSZ(0);
  @$pb.TagNumber(1)
  set tenant($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasTenant() => $_has(0);
  @$pb.TagNumber(1)
  void clearTenant() => $_clearField(1);
}

class ListBindingsResponse extends $pb.GeneratedMessage {
  factory ListBindingsResponse({
    $core.Iterable<RoleBinding>? bindings,
  }) {
    final result = create();
    if (bindings != null) result.bindings.addAll(bindings);
    return result;
  }

  ListBindingsResponse._();

  factory ListBindingsResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ListBindingsResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ListBindingsResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..pPM<RoleBinding>(1, _omitFieldNames ? '' : 'bindings',
        subBuilder: RoleBinding.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListBindingsResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ListBindingsResponse copyWith(void Function(ListBindingsResponse) updates) =>
      super.copyWith((message) => updates(message as ListBindingsResponse))
          as ListBindingsResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ListBindingsResponse create() => ListBindingsResponse._();
  @$core.override
  ListBindingsResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ListBindingsResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ListBindingsResponse>(create);
  static ListBindingsResponse? _defaultInstance;

  @$pb.TagNumber(1)
  $pb.PbList<RoleBinding> get bindings => $_getList(0);
}

class GetBindingRequest extends $pb.GeneratedMessage {
  factory GetBindingRequest({
    $core.String? tenant,
    $core.String? id,
  }) {
    final result = create();
    if (tenant != null) result.tenant = tenant;
    if (id != null) result.id = id;
    return result;
  }

  GetBindingRequest._();

  factory GetBindingRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GetBindingRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GetBindingRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'tenant')
    ..aOS(2, _omitFieldNames ? '' : 'id')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetBindingRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetBindingRequest copyWith(void Function(GetBindingRequest) updates) =>
      super.copyWith((message) => updates(message as GetBindingRequest))
          as GetBindingRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GetBindingRequest create() => GetBindingRequest._();
  @$core.override
  GetBindingRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GetBindingRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GetBindingRequest>(create);
  static GetBindingRequest? _defaultInstance;

  /// Empty ⇒ the caller's own tenant.
  @$pb.TagNumber(1)
  $core.String get tenant => $_getSZ(0);
  @$pb.TagNumber(1)
  set tenant($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasTenant() => $_has(0);
  @$pb.TagNumber(1)
  void clearTenant() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get id => $_getSZ(1);
  @$pb.TagNumber(2)
  set id($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasId() => $_has(1);
  @$pb.TagNumber(2)
  void clearId() => $_clearField(2);
}

class GetBindingResponse extends $pb.GeneratedMessage {
  factory GetBindingResponse({
    RoleBinding? binding,
  }) {
    final result = create();
    if (binding != null) result.binding = binding;
    return result;
  }

  GetBindingResponse._();

  factory GetBindingResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory GetBindingResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'GetBindingResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<RoleBinding>(1, _omitFieldNames ? '' : 'binding',
        subBuilder: RoleBinding.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetBindingResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  GetBindingResponse copyWith(void Function(GetBindingResponse) updates) =>
      super.copyWith((message) => updates(message as GetBindingResponse))
          as GetBindingResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static GetBindingResponse create() => GetBindingResponse._();
  @$core.override
  GetBindingResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static GetBindingResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<GetBindingResponse>(create);
  static GetBindingResponse? _defaultInstance;

  @$pb.TagNumber(1)
  RoleBinding get binding => $_getN(0);
  @$pb.TagNumber(1)
  set binding(RoleBinding value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasBinding() => $_has(0);
  @$pb.TagNumber(1)
  void clearBinding() => $_clearField(1);
  @$pb.TagNumber(1)
  RoleBinding ensureBinding() => $_ensure(0);
}

class PutBindingRequest extends $pb.GeneratedMessage {
  factory PutBindingRequest({
    RoleBinding? binding,
    $core.bool? keepSessions,
  }) {
    final result = create();
    if (binding != null) result.binding = binding;
    if (keepSessions != null) result.keepSessions = keepSessions;
    return result;
  }

  PutBindingRequest._();

  factory PutBindingRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PutBindingRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PutBindingRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<RoleBinding>(1, _omitFieldNames ? '' : 'binding',
        subBuilder: RoleBinding.create)
    ..aOB(2, _omitFieldNames ? '' : 'keepSessions')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PutBindingRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PutBindingRequest copyWith(void Function(PutBindingRequest) updates) =>
      super.copyWith((message) => updates(message as PutBindingRequest))
          as PutBindingRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PutBindingRequest create() => PutBindingRequest._();
  @$core.override
  PutBindingRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PutBindingRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PutBindingRequest>(create);
  static PutBindingRequest? _defaultInstance;

  @$pb.TagNumber(1)
  RoleBinding get binding => $_getN(0);
  @$pb.TagNumber(1)
  set binding(RoleBinding value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasBinding() => $_has(0);
  @$pb.TagNumber(1)
  void clearBinding() => $_clearField(1);
  @$pb.TagNumber(1)
  RoleBinding ensureBinding() => $_ensure(0);

  /// By default a change that takes roles away (replacing a binding with fewer
  /// roles, another subject, or an earlier expiry) revokes the sessions of every
  /// subject the old binding named, so the change applies now rather than at their
  /// next refresh. Set to leave them signed in.
  @$pb.TagNumber(2)
  $core.bool get keepSessions => $_getBF(1);
  @$pb.TagNumber(2)
  set keepSessions($core.bool value) => $_setBool(1, value);
  @$pb.TagNumber(2)
  $core.bool hasKeepSessions() => $_has(1);
  @$pb.TagNumber(2)
  void clearKeepSessions() => $_clearField(2);
}

class PutBindingResponse extends $pb.GeneratedMessage {
  factory PutBindingResponse({
    RoleBinding? binding,
    $core.int? revokedSessions,
  }) {
    final result = create();
    if (binding != null) result.binding = binding;
    if (revokedSessions != null) result.revokedSessions = revokedSessions;
    return result;
  }

  PutBindingResponse._();

  factory PutBindingResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory PutBindingResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'PutBindingResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOM<RoleBinding>(1, _omitFieldNames ? '' : 'binding',
        subBuilder: RoleBinding.create)
    ..aI(2, _omitFieldNames ? '' : 'revokedSessions',
        fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PutBindingResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  PutBindingResponse copyWith(void Function(PutBindingResponse) updates) =>
      super.copyWith((message) => updates(message as PutBindingResponse))
          as PutBindingResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static PutBindingResponse create() => PutBindingResponse._();
  @$core.override
  PutBindingResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static PutBindingResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<PutBindingResponse>(create);
  static PutBindingResponse? _defaultInstance;

  @$pb.TagNumber(1)
  RoleBinding get binding => $_getN(0);
  @$pb.TagNumber(1)
  set binding(RoleBinding value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasBinding() => $_has(0);
  @$pb.TagNumber(1)
  void clearBinding() => $_clearField(1);
  @$pb.TagNumber(1)
  RoleBinding ensureBinding() => $_ensure(0);

  @$pb.TagNumber(2)
  $core.int get revokedSessions => $_getIZ(1);
  @$pb.TagNumber(2)
  set revokedSessions($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasRevokedSessions() => $_has(1);
  @$pb.TagNumber(2)
  void clearRevokedSessions() => $_clearField(2);
}

class DeleteBindingRequest extends $pb.GeneratedMessage {
  factory DeleteBindingRequest({
    $core.String? tenant,
    $core.String? id,
    $core.bool? keepSessions,
  }) {
    final result = create();
    if (tenant != null) result.tenant = tenant;
    if (id != null) result.id = id;
    if (keepSessions != null) result.keepSessions = keepSessions;
    return result;
  }

  DeleteBindingRequest._();

  factory DeleteBindingRequest.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory DeleteBindingRequest.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'DeleteBindingRequest',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOS(1, _omitFieldNames ? '' : 'tenant')
    ..aOS(2, _omitFieldNames ? '' : 'id')
    ..aOB(3, _omitFieldNames ? '' : 'keepSessions')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DeleteBindingRequest clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DeleteBindingRequest copyWith(void Function(DeleteBindingRequest) updates) =>
      super.copyWith((message) => updates(message as DeleteBindingRequest))
          as DeleteBindingRequest;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static DeleteBindingRequest create() => DeleteBindingRequest._();
  @$core.override
  DeleteBindingRequest createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static DeleteBindingRequest getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<DeleteBindingRequest>(create);
  static DeleteBindingRequest? _defaultInstance;

  /// Empty ⇒ the caller's own tenant.
  @$pb.TagNumber(1)
  $core.String get tenant => $_getSZ(0);
  @$pb.TagNumber(1)
  set tenant($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasTenant() => $_has(0);
  @$pb.TagNumber(1)
  void clearTenant() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get id => $_getSZ(1);
  @$pb.TagNumber(2)
  set id($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasId() => $_has(1);
  @$pb.TagNumber(2)
  void clearId() => $_clearField(2);

  /// See `PutBindingRequest.keep_sessions`.
  @$pb.TagNumber(3)
  $core.bool get keepSessions => $_getBF(2);
  @$pb.TagNumber(3)
  set keepSessions($core.bool value) => $_setBool(2, value);
  @$pb.TagNumber(3)
  $core.bool hasKeepSessions() => $_has(2);
  @$pb.TagNumber(3)
  void clearKeepSessions() => $_clearField(3);
}

class DeleteBindingResponse extends $pb.GeneratedMessage {
  factory DeleteBindingResponse({
    $core.bool? deleted,
    $core.int? revokedSessions,
  }) {
    final result = create();
    if (deleted != null) result.deleted = deleted;
    if (revokedSessions != null) result.revokedSessions = revokedSessions;
    return result;
  }

  DeleteBindingResponse._();

  factory DeleteBindingResponse.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory DeleteBindingResponse.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'DeleteBindingResponse',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'agent.v1'),
      createEmptyInstance: create)
    ..aOB(1, _omitFieldNames ? '' : 'deleted')
    ..aI(2, _omitFieldNames ? '' : 'revokedSessions',
        fieldType: $pb.PbFieldType.OU3)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DeleteBindingResponse clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  DeleteBindingResponse copyWith(
          void Function(DeleteBindingResponse) updates) =>
      super.copyWith((message) => updates(message as DeleteBindingResponse))
          as DeleteBindingResponse;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static DeleteBindingResponse create() => DeleteBindingResponse._();
  @$core.override
  DeleteBindingResponse createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static DeleteBindingResponse getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<DeleteBindingResponse>(create);
  static DeleteBindingResponse? _defaultInstance;

  /// False when there was no binding with that id.
  @$pb.TagNumber(1)
  $core.bool get deleted => $_getBF(0);
  @$pb.TagNumber(1)
  set deleted($core.bool value) => $_setBool(0, value);
  @$pb.TagNumber(1)
  $core.bool hasDeleted() => $_has(0);
  @$pb.TagNumber(1)
  void clearDeleted() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.int get revokedSessions => $_getIZ(1);
  @$pb.TagNumber(2)
  set revokedSessions($core.int value) => $_setUnsignedInt32(1, value);
  @$pb.TagNumber(2)
  $core.bool hasRevokedSessions() => $_has(1);
  @$pb.TagNumber(2)
  void clearRevokedSessions() => $_clearField(2);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
