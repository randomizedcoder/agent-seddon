// Native desktop build. Same behaviour as the stub — no browser storage or
// download surface — so we simply reuse it. (If a native file-dialog import is
// ever wanted, implement it here over `dart:io` / `file_selector`.)
export 'graph_platform_stub.dart';
