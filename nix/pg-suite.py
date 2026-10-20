#!/usr/bin/env python3
"""Count named libtest executions against an explicit inventory; sanitize persisted logs."""
import collections
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time


def sanitize(text):
    for key, value in os.environ.items():
        if value and (key.endswith('_DSN') or key.endswith('_TOKEN')):
            text = text.replace(value, '[redacted]')
    return re.sub(r'postgres(?:ql)?://[^\s\"\']+', '[redacted-dsn]', text)


def inventory(text):
    return collections.Counter(re.findall(r'^([^\s]+): test$', text, re.M))


def executed(text):
    # Dev-shell background notices can split a pretty libtest status across lines.
    actual = collections.Counter()
    failures = 0
    pending = None
    for line in text.splitlines():
        start = re.match(r'^test ([^\s]+) \.\.\.\s*(.*)$', line)
        if start:
            pending, status = start.groups()
        else:
            status = line.strip()
        if pending and status in ('ok', 'FAILED', 'ignored'):
            if status != 'ignored':
                actual[pending] += 1
                failures += status == 'FAILED'
            pending = None
    return actual, failures


def emit(directory, suite, status, count, failed, elapsed, reason, command):
    result = dict(suite=suite, status=status, executed=count, failed=failed,
                  elapsed_seconds=round(elapsed, 3), reason=reason, command=command,
                  revision=os.environ.get('AGENT_TEST_REVISION', 'unknown'),
                  pins_sha256=hashlib.sha256(Path('flake.lock').read_bytes()).hexdigest() if Path('flake.lock').exists() else 'unavailable')
    directory.mkdir(parents=True, exist_ok=True)
    temporary = directory / (suite + '.json.tmp')
    temporary.write_text(json.dumps(result, indent=2) + '\n')
    temporary.replace(directory / (suite + '.json'))


def run(command, log):
    # Spool to a file; do not buffer arbitrary test output in memory.
    with log.open('w') as out:
        result = subprocess.run(command, stdout=out, stderr=subprocess.STDOUT, env={**os.environ, 'CARGO_TERM_COLOR': 'never'})
    if log.stat().st_size > 32 * 1024 * 1024:
        log.write_text('output exceeded harness limit\n')
        return 1, ''
    text = sanitize(log.read_text(errors='replace'))
    log.write_text(text)
    return result.returncode, text


def main(argv):
    directory = Path(os.environ['AGENT_PG_ARTIFACT_DIR'])
    directory.mkdir(parents=True, exist_ok=True)
    suite = argv[0]
    if argv[1] == 'record':
        emit(directory, suite, argv[2], 0, 0, 0, argv[3], [])
        return 0
    command = argv[1:]
    if os.environ.get('AGENT_REQUIRE_POSTGRES_TESTS') == '1' and not os.environ.get('AGENT_CONFIG_STORE_TEST_DSN'):
        emit(directory, suite, 'failed', 0, 0, 0, 'required DSN unavailable', command)
        return 1
    started = time.monotonic()
    emit(directory, suite, 'failed', 0, 0, 0, 'incomplete: suite started', command)
    try:
        rc, listing = run(command + ['--list', '--format', 'terse'], directory / (suite + '-inventory.log'))
        expected = inventory(listing)
        if rc or not expected:
            emit(directory, suite, 'failed', 0, 0, time.monotonic()-started, 'inventory failed or zero selected tests', command)
            return 1
        rc, output = run(command + ['--format', 'pretty'], directory / (suite + '.log'))
        actual, failures = executed(output)
        count = sum(actual.values())
        mismatch = actual != expected
        status = 'passed' if not rc and not failures and not mismatch else 'failed'
        reason = 'named execution inventory matched' if status == 'passed' else ('incomplete named executions' if mismatch else 'test assertion failure' if failures else 'test process failed')
        emit(directory, suite, status, count, failures, time.monotonic()-started, reason, command)
        print(f'{suite}: {status}, executed={count}, failed={failures}; artifacts: {directory}')
        return 1 if mismatch or (rc and not failures) else 2 if failures else 0
    except (OSError, KeyboardInterrupt):
        emit(directory, suite, 'failed', 0, 0, time.monotonic()-started, 'harness interrupted or unavailable', command)
        return 1


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
