// Platform split for the graph view's two host-specific needs: a small
// key→string store (the graph library persists here) and file download/upload
// (import/export). Web uses `dart:html` (localStorage + an <a download> / file
// input); native has neither, so it falls back to an in-memory store and inert
// file ops. Same conditional-import idiom as `../transport/channel_factory.dart`.
export 'graph_platform_stub.dart'
    if (dart.library.io) 'graph_platform_io.dart'
    if (dart.library.html) 'graph_platform_web.dart';
