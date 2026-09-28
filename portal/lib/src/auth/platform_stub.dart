import 'auth_platform.dart';

/// Fallback for a platform with neither `dart:io` nor JS interop.
AuthPlatform createAuthPlatform() =>
    throw UnsupportedError('no sign-in platform for this target');
