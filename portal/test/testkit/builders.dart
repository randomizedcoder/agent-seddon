import 'package:agent_portal/src/gen/agent/v1/prompt.pb.dart';

/// Small, readable proto builders for tests — the Dart analogue of
/// `agent-testkit`'s fixtures. Keep args optional with sane defaults so a test
/// names only the fields it cares about.

PromptEntry promptEntry({
  PromptKind kind = PromptKind.PROMPT_KIND_SYSTEM,
  String id = 'p1',
  String content = 'hello',
  bool builtin = false,
  bool readOnly = false,
  int order = 0,
  Iterable<String> tags = const [],
  int version = 1,
}) =>
    PromptEntry(
      kind: kind,
      id: id,
      content: content,
      builtin: builtin,
      readOnly: readOnly,
      order: order,
      tags: tags,
      version: version,
    );

PromptList promptList(List<PromptEntry> entries) =>
    PromptList(entries: entries);
