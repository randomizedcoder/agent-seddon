// promptfoo `javascript` (file://) assertion: does the code the agent WROTE actually
// compile — and, when `expect_output` is given, produce that output when run?
//
// The `agent_provider.sh` wrapper appends the files the agent created to the provider
// output, each as a `===FILE: <relpath>===\n<contents>` block. This assertion pulls out
// the block for `vars.entry`, writes it to a scratch dir, and compiles/runs it with the
// toolchain the nix harness puts on PATH (cc / rustc / go / python3). It is the
// deterministic, ground-truth counterpart to the `llm-rubric` grade.
//
// promptfoo calls this as `(output, context) => GradingResult`. Referenced from
// tasks.yaml as `- type: javascript` / `value: file://assert_compiles.js`.
//
// Required vars: `lang` (c|rust|go|python), `entry` (the filename the agent was asked
// to create). Optional: `expect_output` (exact stdout, one trailing newline tolerated).
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFileSync } = require('node:child_process');

function extractFile(output, entry) {
  // Split on the file markers the wrapper emits; find the block whose header matches.
  const marker = '===FILE: ';
  const idx = output.indexOf(marker + entry + '===');
  if (idx === -1) return null;
  const start = output.indexOf('\n', idx);
  if (start === -1) return null;
  const rest = output.slice(start + 1);
  const next = rest.indexOf('\n' + marker);
  return next === -1 ? rest : rest.slice(0, next);
}

function run(cmd, args, opts = {}) {
  return execFileSync(cmd, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], ...opts });
}

module.exports = (output, context) => {
  const vars = (context && context.vars) || {};
  const lang = String(vars.lang || '').toLowerCase();
  const entry = String(vars.entry || '');
  if (!lang || !entry) {
    return { pass: false, score: 0, reason: 'assert_compiles: test is missing `lang`/`entry` vars' };
  }

  const code = extractFile(output, entry);
  if (code == null) {
    return { pass: false, score: 0, reason: `assert_compiles: the agent did not create ${entry}` };
  }

  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pf-compile-'));
  const src = path.join(dir, entry);
  const prog = path.join(dir, 'prog');
  try {
    fs.writeFileSync(src, code);
    let runProg = null; // [cmd, args]
    switch (lang) {
      case 'c':
        run('cc', [src, '-o', prog]);
        runProg = [prog, []];
        break;
      case 'rust':
        run('rustc', [src, '-o', prog]);
        runProg = [prog, []];
        break;
      case 'go':
        fs.writeFileSync(path.join(dir, 'go.mod'), 'module sol\ngo 1.20\n');
        run('go', ['build', '-o', prog, entry], { cwd: dir, env: { ...process.env, GOCACHE: path.join(dir, '.gocache'), GOFLAGS: '-mod=mod' } });
        runProg = [prog, []];
        break;
      case 'python':
        run('python3', ['-m', 'py_compile', src]); // syntax check
        runProg = ['python3', [src]];
        break;
      default:
        return { pass: false, score: 0, reason: `assert_compiles: unsupported lang '${lang}'` };
    }

    if (vars.expect_output == null) {
      return { pass: true, score: 1, reason: `${lang} source compiles` };
    }

    const got = run(runProg[0], runProg[1], { cwd: dir, timeout: 15000 });
    const want = String(vars.expect_output);
    const ok = got === want || got === want + '\n';
    return ok
      ? { pass: true, score: 1, reason: `${lang} compiles and prints the expected output` }
      : { pass: false, score: 0, reason: `output mismatch: got ${JSON.stringify(got)} want ${JSON.stringify(want)}` };
  } catch (e) {
    const detail = (e && (e.stderr || e.message) || String(e)).toString().slice(0, 400);
    return { pass: false, score: 0, reason: `assert_compiles: ${lang} build/run failed: ${detail}` };
  } finally {
    try { fs.rmSync(dir, { recursive: true, force: true }); } catch { /* best-effort */ }
  }
};
