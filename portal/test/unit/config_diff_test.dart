import 'package:agent_portal/src/config_diff.dart';
import 'package:agent_portal/src/graph_json.dart' show jsonValueToDart;
import 'package:flutter_test/flutter_test.dart';

/// L0 tests for the Settings config diff (`config_diff.dart`) — the pure function
/// behind the tab's dirty check and Save. The staged map is untrusted (it holds
/// whatever a user typed into the schema form, including the raw-JSON escape
/// hatch), so `adversarial_` cases assert the diff stays well-formed and never
/// emits indexed array paths the backend would reject.
void main() {
  // Decode a diff into `{path: value}` for readable assertions.
  Map<String, dynamic> diff(
    Map<String, dynamic> orig,
    Map<String, dynamic> staged, {
    String prefix = 'agent.',
  }) {
    final edits = configEdits(orig, staged, prefix);
    return {for (final e in edits) e.path: jsonValueToDart(e.value)};
  }

  test('positive_changed_scalar_is_one_dotted_edit', () {
    expect(
      diff({'workers': 4}, {'workers': 8}),
      {'agent.workers': 8},
    );
  });

  test('positive_nested_object_change_recurses_path', () {
    expect(
      diff(
        {
          'pool': {'size': 1, 'name': 'a'}
        },
        {
          'pool': {'size': 2, 'name': 'a'}
        },
      ),
      {'agent.pool.size': 2},
    );
  });

  test('corner_no_changes_is_empty', () {
    expect(diff({'a': 1, 'b': 'x'}, {'a': 1, 'b': 'x'}), isEmpty);
  });

  test('corner_new_key_in_staged_is_an_edit', () {
    expect(diff({}, {'added': true}), {'agent.added': true});
  });

  test('negative_key_only_in_orig_is_not_a_deletion', () {
    // The form never removes keys, so a key absent from staged emits nothing.
    expect(diff({'gone': 1}, {}), isEmpty);
  });

  test('boundary_array_change_is_one_atomic_edit', () {
    // A changed list is a single whole-value edit at the key — never `foo[0]`.
    final d = diff(
      {
        'hosts': ['a', 'b']
      },
      {
        'hosts': ['a', 'b', 'c']
      },
    );
    expect(d.keys, ['agent.hosts']);
    expect(d['agent.hosts'], ['a', 'b', 'c']);
  });

  test('boundary_object_to_scalar_is_leaf_edit', () {
    // orig is a Map, staged is a scalar → not both-maps, so a single leaf edit.
    expect(
      diff({'x': {}}, {'x': 5}),
      {'agent.x': 5},
    );
  });

  test('adversarial_int_vs_double_differ_by_json_encoding', () {
    // Documents the jsonEncode-based comparison: 1 and 1.0 encode differently.
    expect(diff({'n': 1}, {'n': 1.0}), {'agent.n': 1.0});
  });

  test('adversarial_deeply_nested_change_only_leaf_emitted', () {
    final orig = {
      'a': {
        'b': {
          'c': {'d': 1}
        }
      }
    };
    final staged = {
      'a': {
        'b': {
          'c': {'d': 2}
        }
      }
    };
    expect(diff(orig, staged), {'agent.a.b.c.d': 2});
  });

  test('adversarial_unicode_key_preserved_in_path', () {
    expect(diff({'ключ': 1}, {'ключ': 2}), {'agent.ключ': 2});
  });

  test('adversarial_null_to_value_and_value_to_null', () {
    expect(diff({'x': null}, {'x': 3}), {'agent.x': 3});
    expect(diff({'y': 3}, {'y': null}), {'agent.y': null});
  });
}
