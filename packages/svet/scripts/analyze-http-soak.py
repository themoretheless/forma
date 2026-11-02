"""Audit HTTP-soak accounting and compare post-warmup memory envelopes.

Budgets are explicit investigation thresholds, not universal runtime limits.
A running checkpoint can never produce a completed qualification.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path

METRICS = {'heap': 'heap', 'rss': 'rss', 'external': 'external',
           'arrayBuffers': 'arrayBuffers'}


def diagnostic_issues(report, complete):
    if 'diagnosticsVersion' not in report:
        return []  # Historical reports have no socket diagnostics.
    if type(report['diagnosticsVersion']) is not int or report['diagnosticsVersion'] != 1:
        return ['unsupported diagnostics version']
    issues = []
    if report.get('coreLifetimeMode') not in (None, '0', '1'):
        issues.append('unsupported core lifetime diagnostic mode')

    def snapshot(label, row):
        if not isinstance(row, dict) or not isinstance(row.get('diagnostics'), dict):
            issues.append(label + ': missing diagnostics')
            return
        d = row['diagnostics']
        if report.get('coreLifetimeMode') == '1':
            life = d.get('requestCoreLifetimes')
            fields = ('created', 'finalized', 'memoryCreated', 'memoryFinalized',
                      'bytesRegistered', 'bytesFinalized', 'maxPendingMemories',
                      'pending', 'pendingMemories', 'pendingRegisteredBytes')
            if not isinstance(life, dict) or any(type(life.get(k)) is not int or life[k] < 0 for k in fields):
                issues.append(label + ': invalid request core lifetime counters')
            elif (life['created'] != life['memoryCreated'] or
                  life['created'] - life['finalized'] != life['pending'] or
                  life['memoryCreated'] - life['memoryFinalized'] != life['pendingMemories'] or
                  life['bytesRegistered'] - life['bytesFinalized'] != life['pendingRegisteredBytes'] or
                  life['maxPendingMemories'] < life['pendingMemories']):
                issues.append(label + ': request core lifetime accounting mismatch')
            elif label == 'afterShutdown' and complete and life['created'] != report['requests']:
                issues.append('request core allocation count does not match completed request accounting')
        sockets, responses = d.get('sockets'), d.get('responses')
        if not isinstance(sockets, dict) or not isinstance(responses, dict):
            issues.append(label + ': invalid socket/response objects')
            return
        values = [sockets.get(k) for k in ('active', 'opened', 'closed', 'readableBytes', 'writableBytes', 'bytesRead', 'bytesWritten')]
        if any(type(v) is not int or v < 0 for v in values):
            issues.append(label + ': invalid socket counters')
        elif sockets['opened'] - sockets['closed'] != sockets['active']:
            issues.append(label + ': socket accounting mismatch')
        active, peak = responses.get('active'), responses.get('peak')
        if type(active) is not int or type(peak) is not int or active < 0 or peak < active:
            issues.append(label + ': invalid response counters')
        resources = d.get('resources')
        if not isinstance(resources, dict) or any(type(v) is not int or v < 0 for v in resources.values()):
            issues.append(label + ': invalid resource counters')
        v8 = d.get('v8')
        if not isinstance(v8, dict):
            issues.append(label + ': invalid V8 object')
            return
        for key in ('externalMemory', 'mallocedMemory', 'peakMallocedMemory', 'totalPhysicalSize'):
            if type(v8.get(key)) is not int or v8[key] < 0:
                issues.append(label + ': invalid V8 counter ' + key)
        if type(d.get('mainWasmBytes')) is not int or d['mainWasmBytes'] <= 0:
            issues.append(label + ': invalid main WASM size')

    for index, row in enumerate(report.get('samples', [])):
        snapshot('sample ' + str(index), row)
    for index, window in enumerate(report.get('windows', [])):
        if window.get('count', 0):
            row = window.get('externalPeakObservation')
            snapshot('window ' + str(index), row)
            if isinstance(row, dict) and row.get('external') != window.get('externalMax'):
                issues.append('window ' + str(index) + ': external peak mismatch')
    if report.get('samples') or complete:
        snapshot('external peak', report.get('externalPeakObservation'))
    if complete:
        for label in ('beforeShutdown', 'afterShutdown'):
            snapshot(label, report.get(label))
        after = report.get('afterShutdown')
        d = after.get('diagnostics') if isinstance(after, dict) else None
        sockets = d.get('sockets') if isinstance(d, dict) else None
        responses = d.get('responses') if isinstance(d, dict) else None
        if not isinstance(sockets, dict) or not isinstance(responses, dict) or sockets.get('active') != 0 or responses.get('active') != 0:
            issues.append('server sockets/responses remain after shutdown')
    return issues


def report_schema_issues(report):
    if not isinstance(report, dict):
        return ['report must be an object']
    issues = []
    for key in ('requests', 'completed', 'disconnected', 'forcedGCs', 'maxNodesAfterFinish'):
        if type(report.get(key)) is not int or report[key] < 0:
            issues.append('invalid nonnegative integer: ' + key)
    for key in ('elapsedMs', 'durationMs'):
        value = report.get(key, 0)
        if type(value) not in (int, float) or not math.isfinite(value) or value < 0:
            issues.append('invalid duration: ' + key)
    for key in ('requestFailures', 'cleanupFailures', 'samples', 'windows'):
        if not isinstance(report.get(key, []), list):
            issues.append('invalid array: ' + key)
    if isinstance(report.get('windows'), list):
        for index, window in enumerate(report['windows']):
            if not isinstance(window, dict) or type(window.get('count')) is not int or window['count'] < 0:
                issues.append('invalid window: ' + str(index))
    return issues


def analyze(report, budgets, minimum_ms):
    issues = report_schema_issues(report)
    if issues:
        return {'status': 'needs-investigation', 'probeCompleted': False,
                'issues': issues, 'metrics': {}, 'minimumDurationMs': minimum_ms,
                'limitations': ['Malformed report cannot establish qualification.']}
    complete = report.get('status') == 'passed'
    issues.extend(diagnostic_issues(report, complete))
    if report.get('requestFailures') or report.get('cleanupFailures'):
        issues.append('request or cleanup failures recorded')
    if report.get('status') not in ('running', 'passed'):
        issues.append('probe did not pass or remain running')
    if report.get('requests', 0) != report.get('completed', 0) + report.get('disconnected', 0):
        issues.append('request accounting mismatch')
    if report.get('maxNodesAfterFinish') != 0:
        issues.append('nonzero graph after request finish')
    if report.get('gcMode') != 'natural' or report.get('forcedGCs') != 0:
        issues.append('natural-GC qualification requires zero forced GCs')
    if complete:
        if report.get('artifactsUnchanged') is not True:
            issues.append('source/runtime identity not verified')
        if report.get('elapsedMs', 0) < report.get('durationMs', math.inf):
            issues.append('requested duration not reached')
        if report.get('elapsedMs', 0) < minimum_ms:
            issues.append('qualification minimum duration not reached')
    windows = report.get('windows', [])
    metrics = {}
    covered = len(windows) == 4 and all(w.get('count', 0) >= 2 for w in windows)
    if complete and not covered:
        issues.append('insufficient coverage of four time windows')
    if covered:
        # Discard the first quarter as warmup. Compare each later quarter to
        # the second, so an intermediate peak cannot be hidden by recovery.
        for name, key in METRICS.items():
            rows = [(w.get(key + 'Min'), w.get(key + 'Max')) for w in windows]
            if any(type(v) not in (int, float) or not math.isfinite(v) or v < 0
                   for pair in rows for v in pair) or any(lo > hi for lo, hi in rows):
                issues.append('invalid memory ranges: ' + name)
                continue
            trough_growth = max(0, *(lo - rows[1][0] for lo, _ in rows[2:]))
            peak_growth = max(0, *(hi - rows[1][1] for _, hi in rows[2:]))
            within = max(trough_growth, peak_growth) <= budgets[name]
            metrics[name] = {'windowRangesBytes': rows,
                             'postWarmupTroughGrowthBytes': trough_growth,
                             'postWarmupPeakGrowthBytes': peak_growth,
                             'growthBudgetBytes': budgets[name], 'withinBudget': within}
            if not within:
                issues.append('post-warmup growth exceeds investigation budget: ' + name)
    status = 'needs-investigation' if issues else ('within-observed-budget' if complete else 'incomplete')
    return {'status': status, 'probeCompleted': complete, 'issues': issues,
            'elapsedMs': report.get('elapsedMs'), 'requests': report.get('requests'),
            'minimumDurationMs': minimum_ms, 'metrics': metrics,
            'limitations': ['Window extrema do not establish a stationary distribution or absence of slow leaks.',
                            'Budgets are caller-specified investigation thresholds, not universal RSS bounds.',
                            'Loopback fixture does not qualify gzip, both codecs, proxy or kernel saturation.']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('--minimum-ms', type=int, default=7200000)
    for metric in METRICS:
        parser.add_argument('--' + metric + '-growth-mib', required=True, type=float)
    args = parser.parse_args()
    budgets = {m: getattr(args, m + '_growth_mib') * 1024 ** 2 for m in METRICS}
    if args.minimum_ms <= 0 or any(not math.isfinite(v) or v < 0 for v in budgets.values()):
        parser.error('duration must be positive and budgets finite/nonnegative')
    raw = args.report.read_bytes()
    try:
        decoded = json.loads(raw)
    except (json.JSONDecodeError, UnicodeDecodeError) as error:
        result = {'status': 'needs-investigation', 'probeCompleted': False,
                  'issues': ['invalid JSON report: ' + str(error)], 'metrics': {}}
    else:
        result = analyze(decoded, budgets, args.minimum_ms)
    result['rawSha256'] = hashlib.sha256(raw).hexdigest()
    result['analyzerSha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    print(json.dumps(result, indent=2))
    return 0 if result['status'] == 'within-observed-budget' else 2


if __name__ == '__main__':
    raise SystemExit(main())
