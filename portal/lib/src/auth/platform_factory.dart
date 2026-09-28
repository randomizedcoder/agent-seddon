// The one place web and native sign-in differ: the web build reads the page URL,
// navigates to the IdP and keeps state in `sessionStorage`; native keeps it in
// memory and cannot redirect. Conditional exports pick `createAuthPlatform`.
export 'platform_stub.dart'
    if (dart.library.io) 'platform_io.dart'
    if (dart.library.js_interop) 'platform_web.dart';
