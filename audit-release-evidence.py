import base64, collections, csv, hashlib, importlib.util, json, shutil, subprocess, sys, tarfile
from pathlib import Path
root=Path('target/release-benchmarks/v1.0.1')
out=Path('/Users/jobz/Works/relayna-gateway-release-evidence/v1.0.1')
out.mkdir(parents=True,exist_ok=True)
source='52c56f9104f43cb108b7ccd647f6911433f8dcfe'
spec=importlib.util.spec_from_file_location('report', 'benches/body-admission/capacity_report.py'); report=importlib.util.module_from_spec(spec);spec.loader.exec_module(report)
data={'schema_version':1,'status':'completed','run':{'harness_commit':source,'architecture':'amd64','resources':{'pods':2,'cpu_per_pod':2,'memory_mib_per_pod':1024,'body_budget_mib_per_pod':512}},'sizes':[],'warnings':['Audited interpretation: only 100 MiB has OOM in all three adjacent trials. Other sizes are finite tested lower bounds. Separate runners and CPU models prevent cross-size or release-to-release speedup claims. Original reports and failed trials are retained unchanged in the raw archive.']}
audit={'runtime_commit':source,'sizes':[],'latency':[],'totals':collections.Counter()}
ids={5:40364072316,10:40364077865,20:40364083515,40:40364089893,80:40364095050,100:40364100050}
for n,id in ids.items():
 p=root/f'full-{n}'/f'body-capacity-amd64-{n}-full'
 r=json.loads((p/'run.json').read_text()); s=json.loads((p/f'{n}mib/summary.json').read_text()); z=report.describe(s)
 assert r['status']=='completed' and not r['errors'] and r['native_verified'] and not r['harness_dirty']
 assert r['image']['source_commit']==source and r['harness_commit']==source and s['complete'] and s['search_mode']=='adaptive'
 selected=[t for t in s['trials'] if t['slots']==z['slots'] and t['cohort_per_pod']==z['cohort_per_pod']]
 adjacent=[t for t in s['trials'] if t['slots']==z['slots']+1 and t['cohort_per_pod']==z['cohort_per_pod']]
 assert len(selected)>=3 and all(t['clean'] and t['resource_stable'] and not t['oom_pods'] and not t['restarts'] and not t['transport_errors'] and t['terminal_zero'] and not t['metric_scrape_errors'] and not t['admission_counter_gap'] and not t['memory_censored'] for t in selected)
 assert all(t['resource_stable'] or t['resource_failure'] for t in s['trials'])
 assert all(not t['warmup_failed'] and not t['incomplete'] and not t['other_http_statuses'] for t in s['trials'])
 assert len(set(pod for t in selected for pod in t['pod_names']))==len(selected)*2
 z['label']='confirmed resource edge' if len(adjacent)>=3 and all(t['oom_pods'] for t in adjacent) else 'tested lower bound'
 data['sizes'].append(z)
 def worst(field,key):return max(t[field][key] for t in selected)
 item={k:v for k,v in z.items() if k!='trials'}
 item.update({'workflow_id':id,'provenance':r,'initial_cohort_per_pod':s['cohort_per_pod'],'offered_cohorts_per_pod':sorted(set(t['cohort_per_pod'] for t in s['trials'])),'selected_directories':[t['directory'] for t in selected],'success_latency_worst_ms':{k:worst('success_latency_ms',k) for k in ['p50','p95','p99']},'rejection_latency_worst_ms':{k:worst('rejection_latency_ms',k) for k in ['p50','p95','p99']},'elapsed_max_ms':max(t['elapsed_ms'] for t in selected),'success_range':[min(t['successes'] for t in selected),max(t['successes'] for t in selected)],'denial_range':[min(t['statuses'].get('503',0) for t in selected),max(t['statuses'].get('503',0) for t in selected)],'total_trials':len(s['trials']),'resource_failure_trials':sum(bool(t['resource_failure']) for t in s['trials']),'memory_censored_trials':sum(bool(t['memory_censored']) for t in s['trials'])})
 audit['sizes'].append(item)
 for t in s['trials']:
  audit['totals'].update({'trials':1,'resource_stable_trials':int(t['resource_stable']),'http_clean_trials':int(t['clean']),'oom_trials':int(bool(t['oom_pods'])),'oom_pods':t['oom_pods'],'restarts':t['restarts'],'successes':t['successes'],'503s':t['statuses'].get('503',0),'transport_errors':t['transport_errors']})
for arm,id in [('baseline-latency',40364111073),('candidate-latency',40364105398)]:
 p=root/arm/'body-capacity-amd64-5-latency';r=json.loads((p/'run.json').read_text());s=json.loads((p/'5mib/summary.json').read_text())
 assert r['status']=='completed' and s['complete'] and len(s['trials'])==3 and all(t['clean'] for t in s['trials'])
 audit['latency'].append({'arm':arm,'workflow_id':id,'provenance':r,'trials':[{k:v for k,v in t.items() if k!='unexpected_results'} for t in s['trials']]})
(out/'gateway-1.0.1-benchmarks.json').write_text(json.dumps(data,indent=2)+'\n')
(out/'gateway-1.0.1-audit.json').write_text(json.dumps(audit,indent=2)+'\n')
with (out/'gateway-1.0.1-trials.csv').open('w',newline='') as f:
 cols=['body_mib',*report.FIELDS];w=csv.DictWriter(f,fieldnames=cols);w.writeheader()
 for s in data['sizes']:
  for t in s['trials']:w.writerow({'body_mib':s['body_mib'],**t})
font=base64.b64encode(Path('docs/assets/brand/fonts/Manrope-4.505.woff2').read_bytes()).decode()
page=report.PAGE.replace('__STATUS__','completed').replace('__DATA__',json.dumps(data,separators=(',',':')).replace('<','\\u003c'))
page=page.replace('</style>',"@font-face{font-family:Manrope;src:url(data:font/woff2;base64,"+font+") format('woff2');font-weight:200 800;font-style:normal}:root{--font-sans:Manrope}button,select,pre,code{font-family:Manrope,sans-serif}</style>")
page=page.replace('Gateway body benchmark','Gateway v1.0.1 · body admission')
page=page.replace('A confirmed resource edge needs at least three stable trials and a repeated adjacent setting with an observed OOM.','This audit calls an edge confirmed only when all three adjacent trials OOM; intermittent adjacent OOMs remain tested lower bounds.')
page=page.replace('<h2>Where is the limit?</h2>', '<div class="note">26 October 2026 · native amd64 · two pods, each 2 CPUs / 1 GiB / 512 MiB body budget. Final confirmations offer 2,048 requests per pod. Selected settings are near resource boundaries and are not safe deployment defaults. <a href="gateway-1.0.1-audit.json">Full provenance and audit</a> · <a href="gateway-1.0.1-benchmarks.json">JSON</a> · <a href="gateway-1.0.1-trials.csv">CSV</a> · <a href="gateway-1.0.1-resource-evidence.tar.gz">Raw resource evidence</a> · <a href="gateway-1.0.1-latency-evidence.tar.gz">Separate latency runs</a></div><h2>Where is the limit?</h2>')
(out/'gateway-1.0.1-benchmarks.html').write_text(page)
shutil.copyfile('docs/assets/brand/fonts/Manrope-OFL.txt',out/'Manrope-OFL.txt')
for name,patterns in [('resource',list(root.glob('full-*'))),('latency',[root/'baseline-latency',root/'candidate-latency'])]:
 with tarfile.open(out/f'gateway-1.0.1-{name}-evidence.tar.gz','w:gz') as t:
  for p in patterns:t.add(p,arcname=p.name)
  t.add('benches/body-admission',arcname='harness/benches/body-admission',filter=lambda info:None if '__pycache__' in info.name else info)
  for p in ['scripts/bench-body-admission','.github/workflows/body-admission-benchmark.yml','Dockerfile','Cargo.toml','Cargo.lock']:
   t.add(p,arcname='harness/'+p)
shutil.copyfile(__file__,out/'audit-release-evidence.py')
for p in sorted(out.iterdir()):
 if p.is_file():assert p.stat().st_size<100*1024*1024,(p,p.stat().st_size)
print(json.dumps({'output':str(out),'totals':audit['totals'],'selected':[(x['body_mib'],x['slots'],x['label'],x['headroom_mib']) for x in audit['sizes']]},indent=2))
