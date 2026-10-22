"""Revalidate primary runs and build the combined, inspectable evidence index.

Run from the repository root after reading the report's reproduction notes.
Raw observations are read only. Requires recorded harness snapshots or Git commits.
"""
import csv
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent / 'amd64'
primary = json.loads((ROOT / 'primary-runs.json').read_text())
assert sorted(item['body_mib'] for item in primary) == [5, 10, 20, 40, 80, 100]
sizes = []
rows = []
fresh_pods = 0
for item in sorted(primary, key=lambda value: value['body_mib']):
    run = item['run_id']
    output = ROOT / 'analyses' / str(run)
    subprocess.run([sys.executable, str(HERE / 'validate.py'),
                    str(ROOT / 'runs' / str(run) / 'full'),
                    '--harness', item['harness_commit'], '--body-mib', str(item['body_mib']),
                    '--run-id', str(run), '--output', str(output)], check=True)
    result = json.loads((output / 'validated-summary.json').read_text())
    assert len(result['sizes']) == 1
    size = result['sizes'][0]
    assert size['stable_trials'] >= 3 and size['adjacent_oom_trials'] > 0
    assert not size['resource_failures_below_chosen']
    size['harness_commit'] = item['harness_commit']
    sizes.append(size)
    fresh_pods += result['fresh_gateway_pods']
    with (output / 'trials.csv').open() as stream:
        for row in csv.DictReader(stream):
            rows.append({'run_id': run, **row})
summary = {
    'validated_at_utc': datetime.now(timezone.utc).isoformat(),
    'runtime_version': '0.1.49',
    'runtime_source_commit': '48c93c07d42b113b8073a403604b111871bf0fda',
    'runtime_image': 'ghcr.io/sarattha/relayna-gateway@sha256:1b40a38f8c5f211d9c5b650a450c4645ee53149d1af7235599afdc133ea69f75',
    'gateway_pods_per_trial': 2,
    'per_pod_limits': {'cpu': 2, 'memory_bytes': 1073741824, 'body_budget_bytes': 536870912},
    'criterion': 'resource_stability; latency and rejection-delivery errors reported separately',
    'primary_trials': len(rows), 'fresh_gateway_pods': fresh_pods, 'sizes': sizes,
}
(ROOT / 'validated-summary.json').write_text(json.dumps(summary, indent=2) + '\n')
with (ROOT / 'trials.csv').open('w') as stream:
    writer = csv.DictWriter(stream, fieldnames=list(rows[0]))
    writer.writeheader()
    writer.writerows(rows)
files = []
for path in sorted((ROOT / 'runs').rglob('*')):
    if path.is_file():
        data = path.read_bytes()
        files.append({'path': str(path.relative_to(ROOT)), 'bytes': len(data),
                      'sha256': hashlib.sha256(data).hexdigest()})
(ROOT / 'raw-evidence-sha256.json').write_text(json.dumps({'files': files}, indent=2) + '\n')
print(f'Validated {len(sizes)} sizes, {len(rows)} trials, {fresh_pods} fresh Gateway pods; indexed {len(files)} raw files.')
