import copy
import importlib.util
from pathlib import Path
import unittest
import json
import subprocess
import sys
import tempfile

spec = importlib.util.spec_from_file_location('http_soak_analyzer', Path(__file__).with_name('analyze-http-soak.py'))
analyzer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(analyzer)


def fixture():
    diagnostics = {'sockets': dict(active=0, opened=2, closed=2, readableBytes=0,
                                  writableBytes=0, bytesRead=100, bytesWritten=200),
                   'responses': dict(active=0, peak=2), 'resources': {},
                   'v8': dict(externalMemory=10, mallocedMemory=2, peakMallocedMemory=3,
                              totalPhysicalSize=100), 'mainWasmBytes': 65536}
    row = {'external': 10, 'diagnostics': diagnostics}
    window = dict(count=2, heapMin=1, heapMax=2, rssMin=3, rssMax=4,
                  externalMin=5, externalMax=10, arrayBuffersMin=1, arrayBuffersMax=2,
                  externalPeakObservation=row)
    return copy.deepcopy(dict(status='passed', gcMode='natural', forcedGCs=0,
        artifactsUnchanged=True, elapsedMs=7200000, durationMs=7200000,
        requests=2, completed=1, disconnected=1, maxNodesAfterFinish=0,
        diagnosticsVersion=1, samples=[copy.deepcopy(row)], windows=[copy.deepcopy(window) for _ in range(4)],
        externalPeakObservation=copy.deepcopy(row), beforeShutdown=copy.deepcopy(row), afterShutdown=copy.deepcopy(row)))


class QualificationTests(unittest.TestCase):
    def analyze(self, report):
        return analyzer.analyze(report, dict.fromkeys(analyzer.METRICS, 64 * 1024**2), 7200000)

    def test_complete_report(self):
        self.assertEqual(self.analyze(fixture())['status'], 'within-observed-budget')

    def test_shutdown_and_counter_corruption_refused(self):
        for mutation in ['leaked_socket', 'mismatched_counters', 'missing_shutdown', 'negative_queue', 'unknown_version']:
            with self.subTest(mutation=mutation):
                report = fixture()
                if mutation == 'leaked_socket':
                    report['afterShutdown']['diagnostics']['sockets'].update(active=1, opened=3)
                elif mutation == 'mismatched_counters':
                    report['afterShutdown']['diagnostics']['sockets']['closed'] = 0
                elif mutation == 'missing_shutdown':
                    del report['afterShutdown']
                elif mutation == 'negative_queue':
                    report['samples'][0]['diagnostics']['sockets']['writableBytes'] = -1
                else:
                    report['diagnosticsVersion'] = 2
                self.assertEqual(self.analyze(report)['status'], 'needs-investigation')

    def test_malformed_nested_objects_refuse_without_crashing(self):
        for field in ['sockets', 'responses', 'v8', 'resources']:
            for invalid in [None, [], 'invalid']:
                with self.subTest(field=field, invalid=invalid):
                    report = fixture()
                    report['afterShutdown']['diagnostics'][field] = invalid
                    self.assertEqual(self.analyze(report)['status'], 'needs-investigation')
        report = fixture(); report['afterShutdown'] = None
        self.assertEqual(self.analyze(report)['status'], 'needs-investigation')

    def test_malformed_report_shape_refuses_without_crashing(self):
        for report in [None, [], 'invalid']:
            self.assertEqual(self.analyze(report)['status'], 'needs-investigation')
        for field, invalid in [('requests', '2'), ('completed', True), ('elapsedMs', None),
                               ('elapsedMs', float('nan')), ('windows', None),
                               ('windows', [None]), ('samples', {}), ('cleanupFailures', 1)]:
            with self.subTest(field=field):
                report = fixture(); report[field] = invalid
                self.assertEqual(self.analyze(report)['status'], 'needs-investigation')

    def test_traced_lifetime_accounting_refuses_corruption(self):
        report = fixture(); report['coreLifetimeMode'] = '1'
        life = dict(created=2, finalized=1, memoryCreated=2, memoryFinalized=1,
                    bytesRegistered=200, bytesFinalized=100, maxPendingMemories=2,
                    pending=1, pendingMemories=1, pendingRegisteredBytes=100)
        rows = report['samples'] + [w['externalPeakObservation'] for w in report['windows']]
        rows += [report['externalPeakObservation'], report['beforeShutdown'], report['afterShutdown']]
        for row in rows:
            row['diagnostics']['requestCoreLifetimes'] = copy.deepcopy(life)
        self.assertEqual(self.analyze(report)['status'], 'within-observed-budget')
        for field, bad in [('created', 3), ('finalized', 5), ('memoryFinalized', 4),
                           ('pendingRegisteredBytes', 0), ('maxPendingMemories', 0), ('pending', True)]:
            with self.subTest(field=field):
                corrupted = copy.deepcopy(report)
                corrupted['afterShutdown']['diagnostics']['requestCoreLifetimes'][field] = bad
                self.assertEqual(self.analyze(corrupted)['status'], 'needs-investigation')
        corrupted = copy.deepcopy(report)
        del corrupted['afterShutdown']['diagnostics']['requestCoreLifetimes']
        self.assertEqual(self.analyze(corrupted)['status'], 'needs-investigation')
        corrupted = copy.deepcopy(report)
        corrupted['requests'] = 3; corrupted['completed'] = 2
        self.assertEqual(self.analyze(corrupted)['status'], 'needs-investigation')

    def test_truncated_json_cli_returns_structured_refusal(self):
        with tempfile.TemporaryDirectory() as folder:
            report = Path(folder) / 'truncated.json'; report.write_text('{')
            command = [sys.executable, str(Path(__file__).with_name('analyze-http-soak.py')), str(report)]
            for metric in analyzer.METRICS:
                command += ['--' + metric + '-growth-mib', '64']
            result = subprocess.run(command, text=True, capture_output=True, check=False)
            self.assertEqual(result.returncode, 2)
            self.assertEqual(json.loads(result.stdout)['status'], 'needs-investigation')
            self.assertNotIn('Traceback', result.stderr)

    def test_failures_cannot_be_hidden_by_passed_status(self):
        for key in ['requestFailures', 'cleanupFailures']:
            report = fixture(); report[key] = [{'message': 'failure'}]
            self.assertEqual(self.analyze(report)['status'], 'needs-investigation')

    def test_running_never_qualifies(self):
        report = fixture(); report['status'] = 'running'; del report['afterShutdown']
        self.assertEqual(self.analyze(report)['status'], 'incomplete')

    def test_external_growth_remains_failure(self):
        report = fixture(); report['windows'][3]['externalMax'] = 100 * 1024**2
        report['windows'][3]['externalPeakObservation']['external'] = 100 * 1024**2
        result = self.analyze(report)
        self.assertIn('post-warmup growth exceeds investigation budget: external', result['issues'])
        self.assertEqual(self.analyze(report)['status'], 'needs-investigation')

    def test_legacy_reports_still_require_duration_and_budgets(self):
        report = fixture(); del report['diagnosticsVersion']; report['elapsedMs'] = 1000
        self.assertEqual(self.analyze(report)['status'], 'needs-investigation')


if __name__ == '__main__':
    unittest.main()
