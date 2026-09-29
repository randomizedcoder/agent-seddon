import '../config.dart';
import 'auth_platform.dart';
import 'cli_login.dart';

/// Fallback for a platform with neither `dart:io` nor JS interop.
AuthPlatform createAuthPlatform() =>
    throw UnsupportedError('no sign-in platform for this target');

/// Only the native desktop signs in through the CLI.
CliLogin? createCliLogin(PortalConfig cfg) => null;
