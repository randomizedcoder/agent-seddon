"""Runner contracts: a successful process alone never proves execution."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

RUNNER = Path(sys.argv.pop(1)).resolve()
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('pg_suite', RUNNER)
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class RunnerTests(unittest.TestCase):
    def invoke(self, mode, required=False, dsn=True):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            fake = directory / 'fake.py'
            fake.write_text("""import sys
mode=sys.argv[1]
if '--list' in sys.argv:
    if mode!='zero': print('case: test')
else:
    if mode not in ('zero','missing'): print('test case ... '+('FAILED' if mode=='failure' else 'ok'))
    if mode=='failure': sys.exit(101)
""")
            env = {**os.environ, 'AGENT_PG_ARTIFACT_DIR': temp, 'AGENT_REQUIRE_POSTGRES_TESTS': '1' if required else '0'}
            env.pop('AGENT_CONFIG_STORE_TEST_DSN', None)
            if dsn:
                env['AGENT_CONFIG_STORE_TEST_DSN'] = 'postgres://secret@host/db'
            result = subprocess.run([sys.executable, str(RUNNER), 'fixture', sys.executable, str(fake), mode], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            artifact = json.loads((directory / 'fixture.json').read_text())
            return result.returncode, artifact

    def test_positive_executed_inventory(self):
        code, result = self.invoke('pass', required=True)
        self.assertEqual((code, result['status'], result['executed']), (0, 'passed', 1))

    def test_negative_zero_tests(self):
        code, result = self.invoke('zero', required=True)
        self.assertEqual((code, result['status'], result['executed']), (1, 'failed', 0))

    def test_corner_missing_execution(self):
        code, result = self.invoke('missing')
        self.assertEqual((code, result['status']), (1, 'failed'))

    def test_negative_assertion(self):
        code, result = self.invoke('failure')
        self.assertEqual((code, result['failed']), (2, 1))

    def test_negative_required_dsn(self):
        code, result = self.invoke('pass', required=True, dsn=False)
        self.assertEqual((code, result['executed']), (1, 0))

    def test_corner_interleaved_status(self):
        actual, failed = runner.executed('test first ... \nbackground notice\nok\ntest next ... ok\n')
        self.assertEqual((sum(actual.values()), failed), (2, 0))

    def test_adversarial_log_secrets(self):
        self.assertNotIn('secret', runner.sanitize('postgres://secret@host/db'))


if __name__ == '__main__':
    unittest.main()
