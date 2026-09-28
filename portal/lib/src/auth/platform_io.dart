import 'auth_platform.dart';

/// Native desktop: no browser to redirect, so sign-in state lives in memory.
AuthPlatform createAuthPlatform() => MemoryAuthPlatform();
