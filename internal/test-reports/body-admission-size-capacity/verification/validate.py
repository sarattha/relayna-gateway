import argparse
import csv
import hashlib
import importlib.util
import json
import re
import tempfile
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument('root', type=Path)
parser.add_argument('--harness', required=True)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--body-mib', nargs='+', type=int, choices=(5,10,20,40,80,100))
parser.add_argument('--run-id', type=int)
args = parser.parse_args()
assert re.fullmatch(r'[0-9a-f]{40}',args.harness), 'pin a full recorded harness commit'
def harness_source(name):
    snapshot = Path(__file__).resolve().parent / 'harnesses' / args.harness / name
    if snapshot.is_file():
        return snapshot.read_bytes()
    return subprocess.check_output(['git', 'show', f'{args.harness}:benches/body-admission/{name}'])
with tempfile.TemporaryDirectory() as temporary:
    module_path=Path(temporary)/'capacity_sweep.py'
    module_path.write_bytes(harness_source('capacity_sweep.py'))
    spec=importlib.util.spec_from_file_location('capacity',module_path)
    capacity=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(capacity)
seen_pods = set()
rows = []
summaries = []
def digest_body(size):
    digest = hashlib.sha256(b'{"payload":"')
    remaining = size - 14
    while remaining:
        count = min(remaining, 65536)
        digest.update(b'x' * count)
        remaining -= count
    digest.update(b'"}')
    return digest.hexdigest()
for summary_path in sorted(args.root.glob('*/summary.json'), key=lambda p: int(p.parent.name.removesuffix('mib'))):
    if args.body_mib and int(summary_path.parent.name.removesuffix('mib')) not in args.body_mib:
        continue
    summary = json.loads(summary_path.read_text())
    assert summary['complete'], summary_path
    size = summary['body_mib']
    expected_digest = digest_body(size * 1048576)
    for name, digest in summary['harness_sha256'].items():
        source = harness_source(name)
        assert hashlib.sha256(source).hexdigest() == digest
    image = json.loads((summary_path.parent/'provenance/release-image.json').read_text())[0]
    assert image['Architecture'] == 'amd64' and image['Os'] == 'linux'
    assert image['Config']['Labels']['org.opencontainers.image.version'] == '0.1.49'
    assert image['Config']['Labels']['org.opencontainers.image.revision'] == summary['source_commit']
    assert (summary_path.parent/'provenance/harness-commit.txt').read_text().strip() == args.harness
    normalized = []
    for original in summary['trials']:
        row = dict(original)
        folder = summary_path.parent/row['directory']
        analyzed = capacity.analyze(folder, row['slots'], row['attempt'], size*1048576, row['cohort_per_pod'])
        for key, value in analyzed.items():
            assert row[key] == value, (size, row['slots'], row['attempt'], key)
        assert not seen_pods.intersection(row['pod_names']), 'Gateway pods reused across trials'
        seen_pods.update(row['pod_names'])
        environment = json.loads((folder/'environment.json').read_text())
        assert environment['gateway_resources'] == {'cpu_limit':2, 'memory_limit_bytes':1073741824}
        assert environment['body_bytes'] == size*1048576
        assert environment['concurrent_per_pod'] == row['cohort_per_pod']
        assert environment['image_architecture'] == 'amd64'
        assert environment['read_delay_ms']==10000 and environment['delay_ms']==2000
        assert environment['client_deadline_ms']==60000
        assert environment['image_allocator_environment']['MALLOC_MMAP_THRESHOLD_']=='131072'
        assert all(node['architecture'] == 'amd64' for node in environment['kubernetes_nodes'])
        if not row['warmup_failed'] and not row['incomplete']:
            assert row['body_sha256'] == expected_digest
        if 'admission_counter_gap' not in row:
            row['admission_counter_gap'] = (row['concurrency_denials'] + row['byte_denials'] - row.get('statuses',{}).get('503',0)) if row.get('concurrency_denials') is not None and row.get('byte_denials') is not None else None
        if 'resource_stable' not in row:
            row['resource_stable'] = bool(not any(row.get(key, True) for key in ('resource_failure','incomplete','warmup_failed','memory_censored','metric_scrape_errors','other_http_statuses')) and row.get('terminal_zero') and row['admission_counter_gap'] == row.get('transport_errors'))
        normalized.append(row)
        record = {key:row.get(key) for key in ['slots','attempt','phase','cohort_per_pod','resource_stable','clean','admission_counter_gap','resource_failure','oom_pods','restarts','previous_container_logs_pods','snapshot_missed_restarts','terminated_pods','warmup_failed','incomplete','memory_censored','memory_streams','rss_peak_mib','cgroup_peak_mib','max_pod_throttled_ms','buffered_requests_peak','buffered_bytes_peak_mib','body_sha256','successes','requests','transport_errors','other_http_statuses','metric_scrape_errors','concurrency_denials','byte_denials','terminal_zero','elapsed_ms']}
        record['body_mib'] = size
        for code in ('200','503','502','0'):
            record['status_'+code] = row.get('statuses',{}).get(code,0)
        for group in ('success_latency_ms','rejection_latency_ms','dispatch_lag_ms'):
            for quantile in ('p50','p95','p99','max'):
                record[group+'_'+quantile] = (row.get(group) or {}).get(quantile)
        rows.append(record)
    final = [r for r in normalized if r['cohort_per_pod'] == summary['final_cohort_per_pod']]
    by_slot = {slot:[r for r in final if r['slots']==slot] for slot in sorted({r['slots'] for r in final})}
    stable = [slot for slot, trials in by_slot.items() if len(trials)>=3 and all(r['resource_stable'] for r in trials)]
    chosen = max(stable, default=None)
    clean_settings = [slot for slot, trials in by_slot.items() if len(trials)>=3 and all(r['clean'] for r in trials)]
    failures = [r for r in final if r['resource_failure']]
    selected = by_slot.get(chosen, [])
    host=(summary_path.parent/'provenance/host.txt').read_text()
    model=next(line.split(':',1)[1].strip() for line in host.splitlines() if line.startswith('Model name:'))
    result = {'cpu_model':model, 'run_id':args.run_id, 'run_url':f'https://github.com/sarattha/relayna-gateway/actions/runs/{args.run_id}' if args.run_id else None, 'body_mib':size, 'arithmetic_complete_bodies':512//size, 'cohort_per_pod':summary['final_cohort_per_pod'],
        'largest_three_resource_stable_slot_setting':chosen, 'largest_three_clean_slot_setting':max(clean_settings, default=None), 'first_observed_resource_failure':min((r['slots'] for r in failures), default=None),
        'final_cohort_tested_slot_range':[min(by_slot),max(by_slot)], 'all_tested_slot_range':[min(r['slots'] for r in normalized),max(r['slots'] for r in normalized)], 'trials':len(summary['trials']),
        'resource_failure_trials':sum(r['resource_failure'] for r in summary['trials']),
        'clean_trials':sum(r['clean'] for r in normalized), 'resource_stable_trials':sum(r['resource_stable'] for r in normalized),
        'stable_rss_peak_mib':max((r['rss_peak_mib'] for r in selected), default=None),
        'stable_cgroup_peak_mib':max((r['cgroup_peak_mib'] for r in selected), default=None),
        'stable_success_p50_max_ms':max((r['success_latency_ms']['p50'] for r in selected if r.get('success_latency_ms')), default=None),
        'stable_success_p99_max_ms':max((r['success_latency_ms']['p99'] for r in selected if r.get('success_latency_ms')), default=None),
        'stable_success_max_ms':max((r['success_latency_ms']['max'] for r in selected if r.get('success_latency_ms')), default=None),
        'stable_rejection_p95_max_ms':max((r['rejection_latency_ms']['p95'] for r in selected if r.get('rejection_latency_ms')), default=None),
        'stable_elapsed_max_ms':max((r['elapsed_ms'] for r in selected), default=None),
        'stable_cpu_throttled_max_ms':max((r['max_pod_throttled_ms'] for r in selected), default=None),
        'stable_per_trial_outcomes':[{key:r.get(key) for key in ('slots','attempt','statuses','successes','concurrency_denials','byte_denials','transport_errors','admission_counter_gap','rss_peak_mib','cgroup_peak_mib','success_latency_ms','rejection_latency_ms','max_pod_throttled_ms')} for r in selected],
        'stable_success_p95_max_ms':max((r['success_latency_ms']['p95'] for r in selected if r.get('success_latency_ms')), default=None),
        'stable_successes_min':min((r.get('successes',0) for r in selected), default=None),
        'stable_successes_max':max((r.get('successes',0) for r in selected), default=None),
        'stable_observed_slots_peak':max((r.get('buffered_requests_peak',0) for r in selected), default=None),
        'stable_observed_bytes_peak_mib':max((r.get('buffered_bytes_peak_mib',0) for r in selected), default=None),
        'stable_trials':len(selected), 'stable_clean_trials':sum(r['clean'] for r in selected),
        'stable_transport_errors':sum(r.get('transport_errors',0) for r in selected),
        'stable_lost_denial_responses':sum(r.get('admission_counter_gap') or 0 for r in selected),
        'stable_other_http_statuses':sum(r.get('other_http_statuses',0) for r in selected),
        'adjacent_setting':chosen+1 if chosen is not None else None,
        'adjacent_trials':len(by_slot.get(chosen+1,[])) if chosen is not None else 0,
        'adjacent_resource_failure_trials':sum(r['resource_failure'] for r in by_slot.get(chosen+1,[])) if chosen is not None else 0,
        'adjacent_oom_trials':sum(bool(r.get('oom_pods')) for r in by_slot.get(chosen+1,[])) if chosen is not None else 0,
        'resource_failures_below_chosen':sorted({r['slots'] for r in failures if chosen is not None and r['slots']<chosen}),
        'confirmation_stop':summary.get('confirmation_stop'), 'refinement_stop':summary.get('refinement_stop'),
        'raw_directory':f"runs/{args.run_id}/full/{size}mib",
        'client_socket_timeout_ms':environment.get('client_socket_timeout_ms',30000),
        'criterion':summary.get('criterion','http_clean_historical'), 'discovery_stop':summary.get('discovery_stop'), 'expansion_stop':summary.get('expansion_stop')}
    summaries.append(result)
    print(json.dumps(result))
args.output.mkdir(parents=True,exist_ok=True)
(args.output/'validated-summary.json').write_text(json.dumps({'harness_commit':args.harness,'sizes':summaries,'fresh_gateway_pods':len(seen_pods),'trials':len(rows)},indent=2)+'\n')
if rows:
    with (args.output/'trials.csv').open('w') as stream:
        writer=csv.DictWriter(stream,fieldnames=list(rows[0]))
        writer.writeheader();writer.writerows(rows)
