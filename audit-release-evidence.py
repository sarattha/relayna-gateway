"""Audit the exact candidate's downloaded release observations before publication."""
import base64
import collections
import csv
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[1]
REPO = ROOT.parents[2]
OUT = ROOT / 'public'
plan = json.loads((ROOT / 'release-plan.json').read_text())
workflows = json.loads((ROOT / 'workflows.json').read_text())
source = plan['runtime_commit']
version = plan['version']
prefix = 'gateway-' + version
OUT.mkdir(exist_ok=True)
spec = importlib.util.spec_from_file_location('capacity_report', REPO / 'benches/body-admission/capacity_report.py')
report = importlib.util.module_from_spec(spec)
spec.loader.exec_module(report)
spec = importlib.util.spec_from_file_location('capacity_sweep', REPO / 'benches/body-admission/capacity_sweep.py')
sweep = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sweep)

def read(path):
    return json.loads(path.read_text())

def save(path, data):
    path.write_text(json.dumps(data, indent=2, allow_nan=False) + '\n')

def observed(values):
    return [v for v in values if isinstance(v, (int, float)) and not isinstance(v, bool)]

def maximum(values):
    return max(observed(values), default=None)

def bounds(values):
    values = observed(values)
    return [min(values), max(values)] if values else [None, None]

def latency(rows, field):
    return {p: maximum((r.get(field) or {}).get(p) for r in rows) for p in ['p50', 'p95', 'p99']}

def validate_run(path, runtime=source):
    run = read(path / 'run.json')
    assert run['status'] == 'completed' and not run['errors'], path
    assert run['native_verified'] and not run['harness_dirty'], path
    assert run['harness_commit'] == source, path
    assert run['architecture'] == plan['architecture'], path
    assert run['resources'] == dict(pods=2, cpu_per_pod=2, memory_mib_per_pod=1024, body_budget_mib_per_pod=512), path
    for filename, expected in run['harness_sha256'].items():
        original = subprocess.check_output(['git', 'show', source + ':benches/body-admission/' + filename], cwd=REPO)
        assert hashlib.sha256(original).hexdigest() == expected, (path, filename)
    if runtime:
        assert run['image']['source_commit'] == runtime, path
    return run

warnings = [
    'Independent native runners may have different CPU models. Historical development studies and preceding release studies are not matched performance controls.',
    'Configured body slots are ceilings, not occupied concurrency. Source and authentication admission remain enabled and can prevent body capacity filling.',
    'The audited edge label requires OOM in every repeated adjacent trial; intermittent adjacent failures remain tested lower bounds. All original observations and failed trials are retained.',
    'Synthetic resource boundaries require production headroom. Successful latency excludes denials and includes the deliberate upstream pauses.'
]
data = dict(schema_version=1, status='completed', run=dict(harness_commit=source, architecture=plan['architecture'], native_verified=True, resources=dict(pods=2, cpu_per_pod=2, memory_mib_per_pod=1024, body_budget_mib_per_pod=512)), sizes=[], warnings=warnings)
audit = dict(runtime_commit=source, version=version, sizes=[], latency=[], totals=collections.Counter(), workflows=workflows, warnings=warnings)
resource_paths = []
for size in plan['full_sizes_mib']:
    matches = [r for r in workflows if r['mode'] == 'full' and r['body_mib'] == size]
    assert len(matches) == 1 and matches[0]['conclusion'] == 'success', (size, matches)
    workflow = matches[0]
    path = ROOT / f'full-{size}' / f'body-capacity-{plan["architecture"]}-{size}-full'
    resource_paths.append(path.parent)
    run = validate_run(path)
    summary = read(path / f'{size}mib/summary.json')
    assert summary['complete'] and summary['search_mode'] == 'adaptive', path
    assert summary['source_commit'] == source and summary['confirmation_repetitions'] >= 3, path
    for row in summary['trials']:
        trial_path = path / f'{size}mib' / row['directory']
        raw = sweep.analyze(trial_path, row['slots'], row['attempt'], size * 1024 * 1024, row['cohort_per_pod'])
        assert all(row.get(key) == value for key, value in raw.items()), (size, row['directory'])
        environment = read(trial_path / 'environment.json')
        assert environment['source_commit'] == source and environment['image_id'] == run['image']['id']
        assert environment['image_architecture'] == plan['architecture']
    description = report.describe(summary)
    selected = [r for r in summary['trials'] if r['slots'] == description['slots'] and r['cohort_per_pod'] == description['cohort_per_pod']]
    adjacent = [r for r in summary['trials'] if r['slots'] == description['slots'] + 1 and r['cohort_per_pod'] == description['cohort_per_pod']]
    assert len(selected) >= 3, (size, len(selected))
    assert all(r['resource_stable'] and not r['oom_pods'] and not r['restarts'] and r['terminal_zero'] and not r['metric_scrape_errors'] and not r['memory_censored'] and r['admission_counter_gap'] == r['transport_errors'] for r in selected), size
    assert all(r['resource_stable'] or r['resource_failure'] for r in summary['trials']), size
    assert all(not r['resource_failure'] or r['oom_pods'] for r in summary['trials']), (size, 'non-OOM restart requires investigation')
    assert all(not r['warmup_failed'] and not r['incomplete'] and not r['other_http_statuses'] for r in summary['trials']), size
    assert len({pod for r in selected for pod in r['pod_names']}) == len(selected) * 2, size
    lower_failure = any(r['resource_failure'] and r['slots'] <= description['slots'] and r['cohort_per_pod'] == description['cohort_per_pod'] for r in summary['trials'])
    description['label'] = 'confirmed resource edge' if len(adjacent) >= 3 and all(r['oom_pods'] for r in adjacent) and not lower_failure else 'tested lower bound'
    data['sizes'].append(description)
    item = {k: v for k, v in description.items() if k != 'trials'}
    item.update(workflow_id=workflow['databaseId'], provenance=run,
        initial_cohort_per_pod=summary['cohort_per_pod'],
        offered_cohorts_per_pod=sorted({r['cohort_per_pod'] for r in summary['trials']}),
        selected_directories=[r['directory'] for r in selected],
        selected_trials=selected,
        success_latency_worst_ms=latency(selected, 'success_latency_ms'),
        rejection_latency_worst_ms=latency(selected, 'rejection_latency_ms'),
        elapsed_max_ms=maximum(r['elapsed_ms'] for r in selected),
        success_range=bounds(r['successes'] for r in selected),
        statuses_range={status: bounds(r['statuses'].get(status, 0) for r in selected) for status in ['200', '429', '503']},
        buffered_requests_peak=maximum(r['buffered_requests_peak'] for r in selected),
        denial_ranges={key: bounds(r.get(key) for r in selected) for key in ['concurrency_denials', 'byte_denials', 'authentication_denials', 'source_denials']},
        total_trials=len(summary['trials']),
        resource_failure_trials=sum(bool(r['resource_failure']) for r in summary['trials']),
        memory_censored_trials=sum(bool(r['memory_censored']) for r in summary['trials']))
    audit['sizes'].append(item)
    for row in summary['trials']:
        audit['totals'].update(dict(trials=1, resource_stable_trials=int(row['resource_stable']), http_clean_trials=int(row['clean']), oom_trials=int(bool(row['oom_pods'])), oom_pods=row['oom_pods'], restarts=row['restarts'], successes=row['successes'], http_503=row['statuses'].get('503', 0), http_429=row['statuses'].get('429', 0), transport_errors=row['transport_errors']))

comparison = [r for r in workflows if r['mode'] == 'comparison']
assert len(comparison) == 1 and comparison[0]['conclusion'] == 'success'
for arm, artifact in [('candidate', 'body-capacity-amd64-5-comparison'), ('baseline-v1.0.2', 'body-baseline-latency-amd64-5')]:
    path = ROOT / 'comparison-5' / artifact
    run = validate_run(path, source if arm == 'candidate' else None)
    summary = read(path / '5mib/summary.json')
    assert summary['complete'] and summary['search_mode'] == 'fixed'
    assert len(summary['trials']) == 3 and all(r['clean'] for r in summary['trials'])
    for row in summary['trials']:
        raw = sweep.analyze(path / '5mib' / row['directory'], row['slots'], row['attempt'], 5 * 1024 * 1024, row['cohort_per_pod'])
        assert all(row.get(key) == value for key, value in raw.items()), (arm, row['directory'])
    assert len({pod for row in summary['trials'] for pod in row['pod_names']}) == 6, arm
    if arm == 'baseline-v1.0.2':
        assert plan['baseline_image'] == run['image']['reference']
    audit['latency'].append(dict(arm=arm, workflow_id=comparison[0]['databaseId'], provenance=run, trials=summary['trials']))

assert audit['latency'][0]['provenance']['preflight']['host'] == audit['latency'][1]['provenance']['preflight']['host']
assert audit['latency'][0]['provenance']['preflight']['docker'] == audit['latency'][1]['provenance']['preflight']['docker']
save(OUT / (prefix + '-benchmarks.json'), data)
save(OUT / (prefix + '-audit.json'), audit)
save(OUT / (prefix + '-workflows.json'), workflows)
with (OUT / (prefix + '-trials.csv')).open('w', newline='') as f:
    writer = csv.DictWriter(f, fieldnames=['body_mib', *report.FIELDS])
    writer.writeheader()
    for size in data['sizes']:
        for row in size['trials']:
            record = dict(body_mib=size['body_mib'], **row)
            record = {k: json.dumps(v, separators=(',', ':')) if isinstance(v, (dict, list)) else v for k, v in record.items()}
            writer.writerow({k: "'" + v if isinstance(v, str) and v.startswith(('=', '+', '-', '@')) else v for k, v in record.items()})

page = report.PAGE.replace('__STATUS__', 'completed').replace('__DATA__', json.dumps(data, separators=(',', ':')).replace('<', '\\u003c'))
font = base64.b64encode((REPO / 'docs/assets/brand/fonts/Manrope-4.505.woff2').read_bytes()).decode()
page = page.replace('</style>', '@font-face{font-family:Manrope;src:url(data:font/woff2;base64,' + font + ') format("woff2");font-weight:200 800}:root{--font-sans:Manrope}button,select,pre,code{font-family:Manrope,sans-serif}</style>')
page = page.replace('Largest stable<br>slots tested', 'Selected repeated<br>stable slots').replace('Largest stable slots tested', 'Selected repeated stable slots')
page = page.replace('Gateway body benchmark', 'Gateway v' + version + ' · body admission')
page = page.replace('A confirmed resource edge needs at least three stable trials and a repeated adjacent setting with an observed OOM.', 'This audit requires OOM in every repeated adjacent trial to label a confirmed edge. Intermittent adjacent failures remain tested lower bounds.')
links = ' · '.join('<a href="' + prefix + suffix + '">' + label + '</a>' for suffix, label in [('-audit.json', 'Audit and provenance'), ('-benchmarks.json', 'JSON'), ('-trials.csv', 'All trials CSV'), ('-resource-evidence.tar.gz', 'Raw resource evidence'), ('-latency-evidence.tar.gz', 'Same-runner latency evidence')])
page = page.replace('<h2>Where is the limit?</h2>', '<p>Native amd64 · two pods, each 2 CPUs / 1 GiB / 512 MiB body budget. Authentication/source controls remain enabled. Configured slots are not occupied concurrency or production recommendations.</p><p>' + links + '</p><h2>Where is the limit?</h2>')
page = page.replace('href="report.json"', 'href="' + prefix + '-benchmarks.json"')
page = page.replace('href="trials.csv"', 'href="' + prefix + '-trials.csv"')
page = page.replace('<a href="run.json">Run checkpoint</a>', '<a href="' + prefix + '-audit.json">All run checkpoints</a>')
page = page.replace('<a href="provision.log">Setup log</a>', '<a href="' + prefix + '-resource-evidence.tar.gz">Raw checkpoints and setup logs</a>')
(OUT / (prefix + '-benchmarks.html')).write_text(page)
shutil.copyfile(REPO / 'docs/assets/brand/fonts/Manrope-OFL.txt', OUT / 'Manrope-OFL.txt')

harness = ROOT / 'harness-snapshot'
harness.mkdir(exist_ok=True)
archive = subprocess.check_output(['git', 'archive', '--format=tar', source, 'benches/body-admission', 'scripts/bench-body-admission', '.github/workflows/body-admission-benchmark.yml', 'Dockerfile', 'Cargo.toml', 'Cargo.lock'], cwd=REPO)
import io
with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
    tar.extractall(harness, filter='data')
for label, paths in [('resource', resource_paths), ('latency', [ROOT / 'comparison-5']), ('superseded-bootstrap', [ROOT / 'superseded-dda8c5d']), ('superseded-zlib', [ROOT / 'superseded-d21547e'])]:
    with tarfile.open(OUT / (prefix + '-' + label + '-evidence.tar.gz'), 'w:gz') as tar:
        for path in paths:
            tar.add(path, arcname=path.name)
        tar.add(harness, arcname='harness')
        if label == 'resource':
            for name in ['release-plan.json', 'dispatches.json', 'workflows.json']:
                tar.add(ROOT / name, arcname=name)
            tar.add(ROOT / 'tooling', arcname='tooling', filter=lambda info: None if '__pycache__' in info.name else info)
security = REPO / 'target/release-verification/v1.2.0/zlib'
with tarfile.open(OUT / (prefix + '-image-security-evidence.tar.gz'), 'w:gz') as tar:
    for name in ['security-provenance.json', 'package-provenance.json', 'debian-package-changelog.txt', 'candidate-image.json', 'candidate-build.log', 'candidate-unfiltered-scan.json', 'candidate-gated-scan.json', 'candidate-runtime-smoke.log', 'gate-regression.json', 'unpatched-gate.json', 'patched-gate.json']:
        assert (security / name).is_file(), name
        tar.add(security / name, arcname=name)
    for name in ['Dockerfile', '.trivyignore-image.yaml', 'docs/security-exceptions.md', 'docs/runtime-image-security.md', 'tests/e2e/runtime-image/run.py']:
        tar.add(REPO / name, arcname='source/' + name)
verification = REPO / 'target/release-verification/v1.2.0'
with tarfile.open(OUT / (prefix + '-verification-evidence.tar.gz'), 'w:gz') as tar:
    for name in ['result.json', 'nextest-isolation-result.json', 'nextest-runtime-equivalence.json', 'auth-nextest-isolated.log']:
        assert (verification / name).is_file(), name
        tar.add(verification / name, arcname=name)
    tar.add(REPO / '.config/nextest.toml', arcname='source/.config/nextest.toml')
shutil.copyfile(__file__, OUT / 'audit-release-evidence.py')
print(json.dumps(dict(output=str(OUT), totals=audit['totals'], selected=[(s['body_mib'], s['slots'], s['label'], s['buffered_requests_peak']) for s in audit['sizes']]), indent=2))
