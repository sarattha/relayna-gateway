import argparse
import json
from pathlib import Path

parser=argparse.ArgumentParser()
parser.add_argument('root',type=Path)
parser.add_argument('--validated',type=Path,required=True)
parser.add_argument('--output',type=Path,required=True)
args=parser.parse_args()
validated=json.loads(args.validated.read_text())
data=[]
for size in validated['sizes']:
    summary=json.loads((args.root/size["raw_directory"]/'summary.json').read_text())
    fields=('slots','cohort_per_pod','resource_stable','clean','transport_errors','admission_counter_gap','resource_failure','oom_pods','rss_peak_mib','cgroup_peak_mib','buffered_requests_peak')
    trials=[{**{key:row.get(key) for key in fields},'success_latency_ms':{'p95':row['success_latency_ms']['p95']} if row.get('success_latency_ms') else None} for row in summary['trials'] if row['cohort_per_pod']==size['cohort_per_pod']]
    for row in trials:
        if row['resource_stable'] is None: row['resource_stable']=row['clean']
    data.append({'summary':size,'trials':trials})
html=r'''<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Gateway body resource sweep</title>
<style>
*{box-sizing:border-box}body{margin:0;color:var(--foreground,#20242b);font:14px/1.5 var(--font-sans,system-ui)}p{margin:0 0 14px}small,.muted{color:var(--muted-foreground,#657080)}.meta{font-size:12px;margin-bottom:16px}table{width:100%;border-collapse:collapse;font-variant-numeric:tabular-nums;font-size:12px}th,td{text-align:right;padding:10px 6px;border-bottom:1px solid var(--border,#d8dee8);vertical-align:top}th{color:var(--muted-foreground,#657080);font-size:11px;font-weight:600}th:first-child,td:first-child{text-align:left}td strong{font-size:16px}td small{display:block;font-size:10px}caption{text-align:left;font-weight:650;font-size:16px;margin-bottom:8px}.note{padding:16px;background:var(--secondary,#f1f4f8);border-radius:var(--radius,10px);font-size:12px;margin-top:16px}.controls{display:flex;gap:6px;flex-wrap:wrap;margin:20px 0 10px}button,select{font:inherit;color:var(--foreground,#20242b);background:var(--card,#fff);border:1px solid var(--border,#d8dee8);border-radius:6px;padding:6px 10px;cursor:pointer}button[aria-pressed=true]{background:var(--primary,#3156c7);color:var(--primary-foreground,#fff);border-color:var(--primary,#3156c7)}button:focus-visible,select:focus-visible{outline:2px solid var(--ring,#6688ee);outline-offset:2px}.plothead{display:flex;justify-content:space-between;align-items:center;gap:8px;margin-bottom:8px}.plothead strong{font-size:13px}.plothead select{max-width:46%;font-size:12px}svg{width:100%;height:270px;display:block;overflow:visible}svg text{font-family:var(--font-sans,system-ui);fill:var(--muted-foreground,#657080);font-size:11px}.grid{stroke:var(--border,#d8dee8);stroke-dasharray:3 4}.limit{stroke:var(--destructive,#d03d42);stroke-dasharray:5 4}.point{cursor:pointer;stroke-width:1.5}.point:focus{outline:none;stroke:var(--foreground,#20242b);stroke-width:3}.legend{display:flex;gap:14px;flex-wrap:wrap;font-size:11px;color:var(--muted-foreground,#657080)}.swatch{display:inline-block;width:8px;height:8px;border-radius:50%;margin-right:5px}.detail{min-height:66px;padding:12px 0;font-size:12px;font-variant-numeric:tabular-nums}.detail strong{font-size:13px}.foot{font-size:11px;color:var(--muted-foreground,#657080);border-top:1px solid var(--border,#d8dee8);padding-top:12px}.bad{color:var(--destructive,#d03d42)}.good{color:var(--success,#16784a)}a{color:var(--info,#3156c7)}
@media(max-width:500px){caption{display:block;width:100%}table,tbody,tr,td{display:block}thead{display:none}tr{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));padding:10px 0;border-bottom:1px solid var(--border,#d8dee8)}td{border:0;padding:3px 0;text-align:left}td:first-child{grid-column:1/-1;margin-bottom:4px}td::before{content:attr(data-label);display:block;font-size:10px;color:var(--muted-foreground,#657080)}td:first-child::before{display:none}td:last-child{grid-column:1/-1}td strong{font-size:15px}td small{display:inline;font-size:10px;margin-left:4px}.plothead{align-items:flex-start}button{padding:6px 9px}.detail{min-height:72px}}
</style></head><body>
<p class="meta">Native amd64 · Two Gateway pods · Each limited to 2 CPUs / 1 GiB · 512 MiB serialized-body budget</p>
<table><caption>Observed resource boundaries</caption><thead><tr><th>JSON</th><th>Complete-body<br>byte bound</th><th>Slots<br>3/3 stable</th><th>Adjacent setting<br>with OOM</th><th>Peak RSS / cgroup</th><th>Success p95</th></tr></thead><tbody id="table"></tbody></table>
<div class="note"><strong id="headroom"></strong><br><strong>Counts are per pod.</strong> Each selected setting passed three fresh-pod trials with complete evidence, verified admitted bodies, returned-to-zero gauges, and no OOM or restart. <span id="selected-summary"></span> Intentional admission 503s are allowed. Latency has no SLO.</div>
<div class="controls" id="sizes" aria-label="JSON body size"></div>
<div class="plothead"><strong id="plot-title"></strong><select id="metric" aria-label="Chart metric"><option value="memory">Peak memory</option><option value="latency">Success p95</option><option value="occupancy">Observed occupied slots</option></select></div>
<p class="muted" id="host" style="font-size:11px;margin-bottom:6px"></p><div id="chart"></div><div class="legend" id="legend"></div><div class="detail" id="detail" aria-live="polite">Select a measured setting to inspect its trials.</div>
<p class="foot">Memory values are the highest individual-pod observation, never the two-pod sum. The resource-stable-setting p95 is the worst per-trial p95 and includes the fixture’s 10-second read pause and 2-second response delay. Failed-run memory peaks may be censored by an OOM. Hosts share four vCPUs with fixtures and differ in CPU model. Cross-size timing includes hardware effects. Charts compare settings at the same final offered cohort; raw evidence also retains earlier discovery loads.<br><a id="run-link" href="https://github.com/sarattha/relayna-gateway/actions/runs/39812510359" target="_blank" rel="noreferrer">Native run and raw artifacts ↗</a> · Published Gateway v0.1.49</p>
<script>
const DATA=__DATA__;
const $=id=>document.getElementById(id), fmt=(n,d=0)=>n==null?'—':Number(n).toFixed(d), sec=n=>n==null?'—':fmt(n/1000,1)+' s';
const edge=DATA.reduce((a,b)=>a.summary.stable_cgroup_peak_mib>b.summary.stable_cgroup_peak_mib?a:b).summary;
$('headroom').textContent='Only '+fmt(1024-edge.stable_cgroup_peak_mib,2)+' MiB minimum observed headroom ('+edge.body_mib+' MiB JSON). These are resource edges, not production settings.';
const total=DATA.reduce((n,d)=>n+d.summary.stable_trials,0),cleanTotal=DATA.reduce((n,d)=>n+d.summary.stable_clean_trials,0);
$('selected-summary').textContent=cleanTotal===total?'All '+total+' selected confirmation trials were HTTP-clean.':cleanTotal+'/'+total+' selected confirmation trials were HTTP-clean; lost denial replies remain reported separately.';
const table=$('table');
for(const d of DATA){const s=d.summary;const tr=document.createElement('tr');tr.innerHTML=`<td><strong>${s.body_mib} MiB</strong><small>${s.cohort_per_pod} offered / pod</small></td><td data-label="Complete-body byte bound">${s.arithmetic_complete_bodies}</td><td data-label="Largest 3/3 resource-stable setting"><strong>${fmt(s.largest_three_resource_stable_slot_setting)}</strong><small>${s.stable_clean_trials}/${s.stable_trials} HTTP-clean${s.stable_lost_denial_responses?"; "+s.stable_lost_denial_responses+" lost denial replies":""}</small></td><td data-label="Adjacent OOM setting" class="${s.adjacent_setting==null?'':'bad'}">${fmt(s.adjacent_setting)}<small>${s.adjacent_setting==null?'not found in tested range':s.adjacent_oom_trials+'/'+s.adjacent_trials+' trials OOM'}</small></td><td data-label="Peak RSS / cgroup">${fmt(s.stable_rss_peak_mib)} / ${fmt(s.stable_cgroup_peak_mib,1)} MiB</td><td data-label="Worst success p95 at stable setting">${sec(s.stable_success_p95_max_ms)}<small>${s.stable_successes_min==null?"":s.stable_successes_min+(s.stable_successes_min===s.stable_successes_max?"":"–"+s.stable_successes_max)+" verified / "+2*s.cohort_per_pod+" offered (both pods)"}</small></td>`;table.append(tr)}
let selected=0;
for(let i=0;i<DATA.length;i++){const b=document.createElement('button');b.textContent=DATA[i].summary.body_mib+' MiB';b.setAttribute('aria-pressed',String(i===selected));b.onclick=()=>{selected=i;document.querySelectorAll('#sizes button').forEach((button,j)=>button.setAttribute('aria-pressed',String(j===i)));draw()};$('sizes').append(b)}
const svgNS='http://www.w3.org/2000/svg';
function el(name,attrs={},text){const element=document.createElementNS(svgNS,name);for(const[k,v]of Object.entries(attrs))element.setAttribute(k,v);if(text!=null)element.textContent=text;return element}
function draw(){
 const d=DATA[selected],s=d.summary,metric=$('metric').value;
 const rows=d.trials.filter(r=>r.cohort_per_pod===s.cohort_per_pod);
 const slots=[...new Set(rows.map(r=>r.slots))].sort((a,b)=>a-b);
 const groups=slots.map(slot=>({slot,trials:rows.filter(r=>r.slots===slot)}));
 const max=(items,key)=>Math.max(0,...items.map(r=>r[key]||0));
 const vals=groups.map(g=>({...g,rss:max(g.trials,'rss_peak_mib'),cgroup:max(g.trials,'cgroup_peak_mib'),occupied:max(g.trials,'buffered_requests_peak'),latency:Math.max(0,...g.trials.filter(r=>r.resource_stable).map(r=>(r.success_latency_ms?.p95||0)/1000)),failed:g.trials.some(r=>r.resource_failure)}));
 $('plot-title').textContent=s.body_mib+' MiB · '+s.cohort_per_pod+' requests / pod';
 $('host').textContent=s.cpu_model||'';
 if(s.run_url)$('run-link').href=s.run_url;
 $('detail').textContent='Select a measured setting to inspect its trials.';
 const width=Math.max(320,$('chart').getBoundingClientRect().width),height=270,left=42,right=18,top=18,bottom=34;
 const xmin=Math.log2(Math.min(...slots)),xmax=Math.log2(Math.max(...slots));
 const x=n=>left+(Math.log2(n)-xmin)/(xmax-xmin||1)*(width-left-right);
 const peak=metric==='memory'?Math.max(1100,...vals.map(v=>v.cgroup)):metric==='latency'?Math.max(15,...vals.map(v=>v.latency))*1.1:Math.max(1,...vals.map(v=>v.occupied))*1.1;
 const y=n=>height-bottom-n/peak*(height-top-bottom);
 const svg=el('svg',{viewBox:`0 0 ${width} ${height}`,role:'img','aria-label':'Measured '+metric+' by configured slot count'});
 for(let i=0;i<=4;i++){const value=peak*i/4;svg.append(el('line',{x1:left,x2:width-right,y1:y(value),y2:y(value),class:'grid'}));svg.append(el('text',{x:left-6,y:y(value)+4,'text-anchor':'end'},fmt(value)))}
 for(let power=Math.ceil(xmin);power<=Math.floor(xmax);power++){const value=2**power;svg.append(el('text',{x:x(value),y:height-15,'text-anchor':'middle'},value))}
 svg.append(el('text',{x:width-right,y:height-1,'text-anchor':'end'},'Configured slots / pod (log₂)'));
 if(metric==='memory'){svg.append(el('line',{x1:left,x2:width-right,y1:y(1024),y2:y(1024),class:'limit'}));svg.append(el('text',{x:width-right,y:y(1024)-5,'text-anchor':'end'},'1 GiB pod limit'))}
 const series=metric==='memory'?[['rss','var(--chart-1,#326bc9)'],['cgroup','var(--chart-2,#c18127)']]:metric==='latency'?[['latency','var(--chart-1,#326bc9)']]:[['occupied','var(--chart-1,#326bc9)']];
 for(const v of vals)for(const[key,color]of series){if(metric==='latency'&&!v.latency)continue;const offset=metric==='memory'?(key==='rss'?-3:3):0;const cx=x(v.slot)+offset,cy=y(v[key]);const point=el('circle',{cx,cy,r:v.failed?5:4,fill:v.failed?'var(--destructive,#d03d42)':color,stroke:v.failed?'var(--destructive,#d03d42)':color,class:'point',tabindex:0,role:'button','aria-label':v.slot+' slots; '+fmt(v[key],1)+(metric==='memory'?' MiB':metric==='latency'?' seconds':' occupied')});
 const show=()=>{const stable=v.trials.filter(r=>r.resource_stable).length,clean=v.trials.filter(r=>r.clean).length,errors=v.trials.reduce((sum,r)=>sum+(r.transport_errors||0),0),oom=v.trials.filter(r=>r.oom_pods).length,other=v.trials.length-clean-v.trials.filter(r=>r.resource_failure).length;const lat=v.trials.filter(r=>r.resource_stable).map(r=>r.success_latency_ms?.p95||0);$('detail').innerHTML=`<strong>${v.slot} configured slots / pod · ${stable}/${v.trials.length} resource-stable trials</strong><br>${oom} trials with OOM; ${clean} HTTP-clean trials; ${errors} client errors across these trials. Peak observed RSS ${fmt(v.rss)} MiB; cgroup ${fmt(v.cgroup)} MiB${v.failed?' (failed-run peak may be censored)':''}. Maximum sampled occupancy ${v.occupied} slots. Worst success p95 at stable setting ${lat.length?sec(Math.max(...lat)):'unavailable'}.`};point.onclick=show;point.onfocus=show;point.onkeydown=e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();show()}};svg.append(point)}
 $('chart').replaceChildren(svg);
 $('legend').innerHTML=metric==='memory'?'<span><i class="swatch" style="background:var(--chart-1,#326bc9)"></i>Process RSS · MiB</span><span><i class="swatch" style="background:var(--chart-2,#c18127)"></i>Cgroup peak · MiB</span><span><i class="swatch" style="background:var(--destructive,#d03d42)"></i>Setting with resource failure</span>':metric==='latency'?'Resource-stable trials only · seconds; highest per-trial p95 at each setting':'Highest sampled occupied-buffer gauge per setting';
}
$('metric').onchange=draw;draw();
requestAnimationFrame(()=>console.log(JSON.stringify({sizes:DATA.length,overflow:document.documentElement.scrollWidth>innerWidth,chartPoints:document.querySelectorAll('.point').length})));
</script></body></html>'''
args.output.write_text(html.replace('__DATA__',json.dumps(data,separators=(',',':')).replace('<','\\u003c')))
