import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('capacity', 'benches/body-admission/capacity_sweep.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

def search(boundary, delayed_failure=False, lost_denial=False, incomplete=False):
    observations = []
    def analyze(folder, slots, attempt, body, cohort):
        failed = slots > boundary or (delayed_failure and slots == boundary and attempt == 3)
        stable = not failed and not (incomplete and slots == boundary)
        row = dict(slots=slots, attempt=attempt, directory=Path(folder).name,
                   cohort_per_pod=cohort, resource_failure=failed,
                   resource_stable=stable, clean=stable and not lost_denial)
        observations.append(row)
        return row
    with tempfile.TemporaryDirectory() as output:
        args = ['capacity', '--kubeconfig', 'fake', '--image', 'fake', '--source-commit', 'fake',
                '--body-mib', '100', '--output', output, '--max-search-slots', '128']
        with patch.object(module, 'analyze', analyze), patch.object(module.subprocess, 'run', return_value=SimpleNamespace(returncode=0)), patch.object(sys, 'argv', args), contextlib.redirect_stdout(io.StringIO()):
            module.main()
        summary = json.loads((Path(output)/'summary.json').read_text())
    return summary, observations

s, rows = search(37, lost_denial=True)
assert any(r['slots'] == 64 for r in rows), 'lost denial prevented expansion'
assert all(r['resource_stable'] for r in rows if r['slots'] == 37)
assert len([r for r in rows if r['slots'] == 37]) == 3
assert len([r for r in rows if r['slots'] == 38]) == 3
assert s['criterion'] == 'resource_stability'
print('PASS accounted lost denials do not stop expansion or confirmation')
s, rows = search(32, delayed_failure=True)
assert any(r['slots'] == 32 and r['resource_failure'] for r in rows)
assert len([r for r in rows if r['slots'] == 31]) == 3
assert all(r['resource_stable'] for r in rows if r['slots'] == 31)
print('PASS confirmation-only resource failures trigger repeated refinement')
s, rows = search(128)
assert s['expansion_stop'].startswith('finite search bound')
assert not any(r['resource_failure'] for r in rows)
print('PASS finite search ceiling remains a lower bound without a resource cliff')
s, rows = search(37, incomplete=True)
assert not any(r['resource_stable'] for r in rows if r['slots'] == 37)
assert s['confirmation_stop'].startswith('non-resource failure')
print('PASS missing admitted responses cannot become resource-stable trials')

# Replay real immutable failures and controls against the current analyzer.
raw = Path(__file__).resolve().parents[1] / 'amd64' / 'runs'
cases = [
    (39825735001, 100, 64, 1, 128, True, False),
    (39820686185, 20, 169, 3, 1024, True, False),
    (39820680177, 10, 128, 2, 1024, False, False),
    (39812510359, 5, 128, 3, 1024, True, True),
]
for run, size, slots, attempt, cohort, stable, clean in cases:
    folder = raw / str(run) / 'full' / f'{size}mib' / f'slots-{slots:04}-attempt-{attempt}'
    result = module.analyze(folder, slots, attempt, size * 1048576, cohort)
    assert (result['resource_stable'], result['clean']) == (stable, clean)
    print(f'PASS real {size} MiB run {run}, slots {slots}, attempt {attempt}: resource_stable={stable}, clean={clean}')
