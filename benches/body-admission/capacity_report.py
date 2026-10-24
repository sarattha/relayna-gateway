"""Offline, dependency-free HTML/JSON/CSV reports from benchmark checkpoints."""

import csv
import hashlib
import html
import json
import math
from pathlib import Path

FIELDS = ("directory", "slots", "attempt", "cohort_per_pod", "phase", "status", "resource_failure",
          "client_deadline_ms", "condition_timeout_seconds", "trial_timeout_seconds",
          "resource_stable", "clean", "incomplete", "memory_censored", "memory_streams",
          "oom_pods", "terminated_pods", "restarts", "transport_errors", "admission_counter_gap",
          "successes", "requests", "statuses", "rss_peak_mib", "cgroup_peak_mib",
          "buffered_requests_peak", "buffered_bytes_peak_mib", "elapsed_ms", "max_pod_throttled_ms",
          "success_latency_ms", "rejection_latency_ms", "concurrency_denials", "byte_denials", "analysis_error")


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")
    temporary.replace(path)


def source_hashes(directory):
    return {path.name: hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(directory.glob("*.py")) + sorted(directory.glob("*.mjs"))}


def number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def peak(rows, key):
    values = [row[key] for row in rows if number(row.get(key))]
    return max(values) if values else None


def normalize(row):
    result = {key: row.get(key) for key in FIELDS}
    # Older checkpoints predate the resource_stable field; their clean flag is
    # stricter. Do not infer a pass from missing evidence.
    result["resource_stable"] = bool(row.get("resource_stable", row.get("clean", False)))
    if row.get("memory_streams") == 0:
        result["rss_peak_mib"] = result["cgroup_peak_mib"] = None
    for key in ("success_latency_ms", "rejection_latency_ms"):
        result[key] = {p: value for p, value in (row.get(key) or {}).items() if number(value)}
    return result


def describe(summary):
    rows = [normalize(row) for row in summary.get("trials", [])]
    cohort = summary.get("final_cohort_per_pod") or peak(rows, "cohort_per_pod")
    current = [row for row in rows if row["cohort_per_pod"] == cohort]
    groups = {slot: [row for row in current if row["slots"] == slot]
              for slot in {row["slots"] for row in current if number(row["slots"]) and row["slots"] > 0}}
    stable = [slot for slot, trials in groups.items() if all(row["resource_stable"] for row in trials)]
    candidate = max(stable, default=None)
    selected = groups.get(candidate, [])
    adjacent = groups.get(candidate + 1, []) if candidate is not None else []
    complete = bool(summary.get("complete")) and not any(summary.get(key) for key in ("error", "refinement_stop", "confirmation_stop"))
    confirmed = (complete and len(selected) >= 3 and len(adjacent) >= 3
                 and any(row.get("oom_pods", 0) for row in adjacent)
                 and not any(row["resource_failure"] and row["slots"] <= candidate for row in current)) if candidate is not None else False
    fixed = summary.get("search_mode") == "fixed"
    label = "confirmed resource edge" if confirmed else "tested setting" if fixed and complete else "tested lower bound" if complete and candidate is not None else "incomplete"
    rss, cgroup = peak(selected, "rss_peak_mib"), peak(selected, "cgroup_peak_mib")
    p95 = peak([{"p95": row["success_latency_ms"].get("p95")} for row in selected], "p95")
    return {"body_mib": summary.get("body_mib"), "complete": complete, "label": label,
            "client_deadline_ms": summary.get("client_deadline_ms"),
            "condition_timeout_seconds": summary.get("condition_timeout_seconds"),
            "trial_timeout_seconds": summary.get("trial_timeout_seconds"),
            "cohort_per_pod": cohort, "slots": candidate, "stable_trials": len(selected),
            "http_clean_trials": sum(bool(row["clean"]) for row in selected),
            "adjacent_slots": candidate + 1 if adjacent else None,
            "adjacent_trials": len(adjacent), "adjacent_oom_trials": sum(bool(row.get("oom_pods")) for row in adjacent),
            "rss_peak_mib": rss, "cgroup_peak_mib": cgroup,
            "headroom_mib": 1024 - cgroup if cgroup is not None else None,
            "success_p95_ms": p95, "error": summary.get("error") or summary.get("refinement_stop") or summary.get("confirmation_stop"),
            "trials": rows}


def collect(root):
    warnings = []

    def read(path):
        try:
            value = json.loads(path.read_text())
            if not isinstance(value, dict):
                raise ValueError("expected an object")
            return value
        except (ValueError, OSError) as error:
            warnings.append(f"{path.relative_to(root)}: {type(error).__name__}: {error}")
            return {}

    meta = read(root / "run.json") if (root / "run.json").exists() else {}
    paths = sorted(root.glob("*mib/summary.json"))
    if (root / "summary.json").exists():
        paths.append(root / "summary.json")
    sizes = []
    for path in paths:
        summary = read(path)
        inferred = path.parent.name.removesuffix("mib")
        if "body_mib" not in summary and inferred.isdigit():
            summary["body_mib"] = int(inferred)
        try:
            sizes.append(describe(summary))
        except (TypeError, ValueError, KeyError, AttributeError) as error:
            warnings.append(f"{path.relative_to(root)}: invalid checkpoint: {error}")
            sizes.append(describe({"body_mib": summary.get("body_mib")}))
    seen = {size["body_mib"] for size in sizes}
    for size in meta.get("body_mib", []):
        if size not in seen:
            sizes.append({**describe({"body_mib": size}), "label": "not run"})
    sizes.sort(key=lambda size: size["body_mib"] or 0)
    status = meta.get("status", "completed" if sizes and all(size["complete"] for size in sizes) else "incomplete")
    if warnings or (status == "completed" and any(not size["complete"] for size in sizes)):
        status = "incomplete" if status == "completed" else status
    if status == "running":
        status = "in progress / unfinished"
    return {"schema_version": 1, "status": status, "run": meta, "sizes": sizes, "warnings": warnings}


def generate(root):
    root = Path(root)
    if not root.is_dir() or not ((root / "run.json").exists() or list(root.glob("*mib/summary.json")) or (root / "summary.json").exists()):
        raise ValueError(f"no benchmark checkpoint in {root}")
    data = collect(root)
    write_json(root / "report.json", data)
    columns = ["body_mib", *[key for key in FIELDS if key not in ("statuses", "success_latency_ms", "rejection_latency_ms")],
               "success_p50_ms", "success_p95_ms", "success_p99_ms", "rejection_p95_ms", "http_503"]
    with (root / "trials.csv").open("w", newline="") as output:
        writer = csv.DictWriter(output, fieldnames=columns, extrasaction="ignore")
        writer.writeheader()
        for size in data["sizes"]:
            for row in size["trials"]:
                # Prefix spreadsheet formula text without modifying raw evidence.
                record = {key: "'" + value if isinstance(value, str) and value.startswith(("=", "+", "-", "@")) else value for key, value in row.items()}
                writer.writerow({**record, "body_mib": size["body_mib"],
                                 **{f"success_{p}_ms": row["success_latency_ms"].get(p) for p in ("p50", "p95", "p99")},
                                 "rejection_p95_ms": row["rejection_latency_ms"].get("p95"),
                                 "http_503": (row["statuses"] or {}).get("503")})
    payload = json.dumps(data, allow_nan=False, separators=(",", ":")).replace("<", "\\u003c")
    (root / "report.html").write_text(PAGE.replace("__STATUS__", html.escape(data["status"])).replace("__DATA__", payload))
    return {"schema_version": 1, "status": data["status"], "report": str(root / "report.html"), "warnings": data["warnings"]}


PAGE = r'''<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Gateway body benchmark</title><style>
*{box-sizing:border-box}html{color-scheme:light dark}body{background:var(--background,#fff);margin:0;color:var(--foreground,#20242b);font:14px/1.5 var(--font-sans,system-ui)}main{max-width:1100px;margin:auto;padding:24px}h1{font-size:23px;line-height:1.2;margin:0 0 8px}h2{font-size:16px;margin:0 0 10px}p{margin:0 0 12px}.muted,small{color:var(--muted-foreground,#667183)}.banner,.note{background:var(--secondary,#f0f3f8);padding:14px;border-radius:var(--radius,8px);margin:16px 0}.banner{border-left:4px solid var(--info,#356ad0)}.banner[data-status=failed],.banner[data-status=interrupted]{border-color:var(--destructive,#d13f49)}.status{text-transform:uppercase;font-weight:700;font-size:12px;letter-spacing:.05em}.errors{white-space:pre-wrap;overflow-wrap:anywhere;font-size:12px;margin:6px 0 0}.meta{font-size:12px;overflow-wrap:anywhere}table{border-collapse:collapse;width:100%;font-size:12px;font-variant-numeric:tabular-nums}th,td{text-align:right;padding:10px 6px;border-bottom:1px solid var(--border,#d8dee8);vertical-align:top}th{color:var(--muted-foreground,#667183);font-size:11px}th:first-child,td:first-child{text-align:left}td strong{font-size:17px}td small{display:block;font-size:11px}.controls,.plothead{display:flex;gap:7px;flex-wrap:wrap;align-items:center;margin:16px 0 10px}.plothead{justify-content:space-between}.plothead h2{margin:0}button,select{font:inherit;padding:6px 10px;color:var(--foreground,#20242b);background:var(--card,#fff);border:1px solid var(--border,#d8dee8);border-radius:6px;cursor:pointer}button[aria-pressed=true]{background:var(--primary,#3156c7);color:var(--primary-foreground,#fff)}button:focus-visible,select:focus-visible{outline:2px solid var(--ring,#6688ee);outline-offset:2px}svg{width:100%;height:270px;display:block}svg text{font:11px var(--font-sans,system-ui);fill:var(--muted-foreground,#667183)}.grid{stroke:var(--border,#d8dee8);stroke-dasharray:3 4}.point{cursor:pointer}.point:focus{outline:none;stroke:var(--foreground,#20242b);stroke-width:3}.detail{min-height:85px;padding:12px 0;font-size:12px;white-space:pre-wrap;overflow-wrap:anywhere}.legend{font-size:11px;color:var(--muted-foreground,#667183)}.note{font-size:12px}a{color:var(--info,#3156c7)}details{font-size:12px;border-top:1px solid var(--border,#d8dee8);padding-top:12px}pre{white-space:pre-wrap;overflow-wrap:anywhere;font-size:11px}footer{font-size:12px;margin-top:16px}.bad{color:var(--destructive,#d13f49)}
@media(max-width:550px){main{padding:16px}table,tbody,tr,td{display:block}thead{display:none}tr{display:grid;grid-template-columns:1fr 1fr;border-bottom:1px solid var(--border,#d8dee8);padding:8px 0}td{border:0;text-align:left;padding:4px 0}td:first-child{grid-column:1/-1}td:before{content:attr(data-label);display:block;font-size:10px;color:var(--muted-foreground,#667183)}td:first-child:before{display:none}.plothead{align-items:flex-start}.plothead select{max-width:55%;font-size:12px}h1{font-size:21px}}
@media(prefers-color-scheme:dark){body{background:var(--background,#0f1725);--foreground:#e4e9f2;--muted-foreground:#a7b0bf;--secondary:#202a3b;--card:#161e2d;--border:#354052;--info:#84acff;--destructive:#ff7c85}}
</style></head><body><main>
<h1>Gateway body benchmark</h1><p class="meta" id="meta"></p>
<div class="banner" id="banner"><span class="status">__STATUS__</span><p id="outcome" style="margin:5px 0 0"></p><div id="errors" class="errors"></div></div>
<h2>Where is the limit?</h2><p class="muted" style="font-size:12px">Slots and memory are per pod. A confirmed resource edge needs at least three stable trials and a repeated adjacent setting with an observed OOM. A tested setting or lower bound does not establish the limit.</p>
<table><thead><tr><th>JSON / load</th><th>Largest stable<br>slots tested</th><th>Evidence</th><th>Peak RSS / cgroup<br>MiB</th><th>Memory headroom<br>MiB</th><th>Success p95</th></tr></thead><tbody id="summary"></tbody></table>
<div id="sizes" class="controls" aria-label="JSON body size"></div><div class="plothead"><h2 id="title">Measurements</h2><select id="metric" aria-label="Chart metric"><option value="memory">Memory · MiB</option><option value="latency">Success p95 · seconds</option><option value="occupancy">Occupied slots</option></select></div>
<p class="meta muted" id="cohort"></p><div id="chart"></div><p class="legend">Blue: RSS / selected metric · Gold: cgroup peak · Red: setting with resource failure · Amber: incomplete or non-resource error. Points summarize trials at the final offered cohort. Select a point for details.</p><div class="detail" id="detail" aria-live="polite"></div>
<div class="note"><strong>Resource edges need production headroom.</strong> The 512 MiB byte budget counts admitted serialized body bytes. Total pod memory also includes connection state, parsers, buffers and allocator overhead. floor(512 / JSON MiB) is the complete-body arithmetic bound, not the configured-slot limit. Most requests in this overload experiment may intentionally receive 503.<br>Latency includes a 10-second upstream read pause and a 2-second post-upload delay; no latency SLO is imposed. Memory is the largest individual-pod observation. OOM peaks may be censored, and absent data is shown as unavailable. Compare runs on the same architecture and host resources.</div>
<details><summary>Run provenance and methodology</summary><pre id="provenance"></pre></details><footer><a href="report.json">Report JSON</a> · <a href="trials.csv">All trial measurements · CSV</a> · <a href="run.json">Run checkpoint</a> · <a href="provision.log">Setup log</a></footer>
</main><script>
const DATA=__DATA__, $=id=>document.getElementById(id), finite=n=>typeof n==='number'&&Number.isFinite(n), fmt=(n,d=0)=>finite(n)?n.toFixed(d):'—', sec=n=>finite(n)?fmt(n/1000,2)+' s':'—';
const run=DATA.run, resource=run.resources||{}, arch=run.architecture||'architecture not recorded';
$('meta').textContent=(run.native_verified?'Native ':'')+arch+' · Two pods, each 2 CPUs / 1 GiB · 512 MiB body budget per pod';
$('banner').dataset.status=DATA.status;
$('outcome').textContent=DATA.status==='completed'?'Measurement finished. Expected overload rejections and observed OOM trials remain part of the results.':DATA.status==='in progress / unfinished'?'Checkpoint captured before completion. If the process stopped, regenerate this report after recovering available evidence.':'Measurement did not finish. Available trials remain visible; this run does not establish a usable limit.';
$('errors').textContent=[...(run.errors||[]).map(e=>e.stage+': '+e.message),...DATA.warnings,...DATA.sizes.filter(s=>s.error).map(s=>s.body_mib+' MiB: '+s.error)].join('\n');
$('provenance').textContent=JSON.stringify({run,notes:'Each trial uses fresh pods, verified exact JSON bodies and per-pod metrics. Resource stability requires no termination/restart, complete memory and metrics, returned-to-zero admission gauges, and reconciliation of every request to a verified success or server denial. Lost denial responses are reported separately from HTTP-clean results. Earlier discovery cohorts are retained in CSV/JSON.'},null,2);
function cell(tr,label,text,small){const td=document.createElement('td');td.dataset.label=label;td.textContent=text;if(small){const el=document.createElement('small');el.textContent=small;td.append(el)}tr.append(td)}
for(const s of DATA.sizes){const tr=document.createElement('tr');cell(tr,'JSON',s.body_mib+' MiB',fmt(s.cohort_per_pod)+' offered / pod');cell(tr,'Largest stable slots tested',fmt(s.slots),s.label);cell(tr,'Evidence',s.stable_trials+' stable; '+s.http_clean_trials+' HTTP-clean',s.adjacent_slots?`${s.adjacent_slots} slots: ${s.adjacent_oom_trials}/${s.adjacent_trials} trials OOM`:'Adjacent OOM not established');cell(tr,'Peak RSS / cgroup',fmt(s.rss_peak_mib,1)+' / '+fmt(s.cgroup_peak_mib,1));cell(tr,'Memory headroom',fmt(s.headroom_mib,2));cell(tr,'Worst per-trial success p95',sec(s.success_p95_ms));$('summary').append(tr)}
let selected=0;for(let i=0;i<DATA.sizes.length;i++){const b=document.createElement('button');b.textContent=DATA.sizes[i].body_mib+' MiB';b.setAttribute('aria-pressed',String(i===selected));b.onclick=()=>{selected=i;document.querySelectorAll('#sizes button').forEach((b,j)=>b.setAttribute('aria-pressed',String(i===j)));draw()};$('sizes').append(b)}
const NS='http://www.w3.org/2000/svg';function el(name,attrs,text){const node=document.createElementNS(NS,name);for(const[k,v]of Object.entries(attrs||{}))node.setAttribute(k,v);if(text!=null)node.textContent=text;return node}
const maximum=values=>{values=values.filter(finite);return values.length?Math.max(...values):null};
function draw(){const s=DATA.sizes[selected], metric=$('metric').value;$('chart').replaceChildren();$('detail').textContent='';if(!s){$('chart').textContent='No trials were started.';return}$('title').textContent=s.body_mib+' MiB JSON';$('cohort').textContent='Final offered cohort: '+fmt(s.cohort_per_pod)+' requests / pod · Complete-body arithmetic bound: '+Math.floor(512/s.body_mib)+' / pod';
 const rows=s.trials.filter(r=>r.cohort_per_pod===s.cohort_per_pod&&finite(r.slots)&&r.slots>0),slots=[...new Set(rows.map(r=>r.slots))].sort((a,b)=>a-b);if(!slots.length){$('chart').textContent='No trial measurements available.';return}
 const width=Math.max(280,$('chart').getBoundingClientRect().width),height=270,left=44,right=14,top=20,bottom=38,xmin=Math.log2(slots[0]),xmax=Math.log2(slots.at(-1)),x=n=>xmin===xmax?(width+left-right)/2:left+(Math.log2(n)-xmin)/(xmax-xmin)*(width-left-right);
 const groups=slots.map(slot=>{const trials=rows.filter(r=>r.slots===slot);return{slot,trials,rss:maximum(trials.map(r=>r.rss_peak_mib)),cgroup:maximum(trials.map(r=>r.cgroup_peak_mib)),latency:maximum(trials.filter(r=>r.resource_stable).map(r=>finite(r.success_latency_ms.p95)?r.success_latency_ms.p95/1000:null)),occupancy:maximum(trials.map(r=>r.buffered_requests_peak)),failed:trials.some(r=>r.resource_failure),invalid:trials.some(r=>!r.resource_failure&&!r.resource_stable)}});
 const series=metric==='memory'?[['rss','#3978d3'],['cgroup','#bb862e']]:[[metric,'#3978d3']],peak=metric==='memory'?Math.max(1100,...groups.flatMap(g=>[g.rss||0,g.cgroup||0])):Math.max(1,...groups.map(g=>g[metric]||0))*1.1,y=n=>height-bottom-n/peak*(height-top-bottom),svg=el('svg',{viewBox:`0 0 ${width} ${height}`,role:'img','aria-label':'Measured '+metric+' by slots'});
 for(let i=0;i<=4;i++){const n=peak*i/4;svg.append(el('line',{x1:left,x2:width-right,y1:y(n),y2:y(n),class:'grid'}));svg.append(el('text',{x:left-6,y:y(n)+4,'text-anchor':'end'},fmt(n)))}
 const ticks=slots.length===1?slots:Array.from({length:Math.floor(xmax)-Math.ceil(xmin)+1},(_,i)=>2**(Math.ceil(xmin)+i));for(const n of ticks)svg.append(el('text',{x:x(n),y:height-18,'text-anchor':'middle'},n));svg.append(el('text',{x:width-right,y:height-1,'text-anchor':'end'},'Configured slots / pod (log₂)'));
 if(metric==='memory'){svg.append(el('line',{x1:left,x2:width-right,y1:y(1024),y2:y(1024),stroke:'#d13f49','stroke-dasharray':'5 4'}));svg.append(el('text',{x:width-right,y:y(1024)-5,'text-anchor':'end'},'1 GiB limit'))}
 for(const g of groups)for(const[key,color]of series){if(!finite(g[key]))continue;const point=el('circle',{cx:x(g.slot)+(metric==='memory'?(key==='rss'?-3:3):0),cy:y(g[key]),r:5,fill:g.failed?'#d13f49':g.invalid?'#bb862e':color,class:'point',tabindex:0,role:'button','aria-label':g.slot+' slots; '+key+' '+fmt(g[key],2)});const show=()=>{$('detail').textContent=`${g.slot} slots / pod · ${g.trials.filter(r=>r.resource_stable).length}/${g.trials.length} resource-stable · ${g.trials.filter(r=>r.clean).length} HTTP-clean\nPeak RSS ${fmt(g.rss,1)} MiB; cgroup ${fmt(g.cgroup,1)} MiB${g.trials.some(r=>r.memory_censored)?' (censored/incomplete peaks)':''}. Occupied slots ${fmt(g.occupancy)}. Worst stable success p95 ${finite(g.latency)?fmt(g.latency,2)+' s':'unavailable'}.\nBoth pods, all trials: ${g.trials.reduce((n,r)=>n+(r.successes||0),0)} verified successes; ${g.trials.reduce((n,r)=>n+((r.statuses||{})['503']||0),0)} HTTP 503; ${g.trials.reduce((n,r)=>n+(r.transport_errors||0),0)} transport errors; ${g.trials.filter(r=>r.oom_pods).length} OOM trials.\n`+g.trials.filter(r=>r.analysis_error).map(r=>r.analysis_error).join('\n')};point.onclick=show;point.onfocus=show;point.onkeydown=e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();show()}};svg.append(point)}$('chart').append(svg);
 if(!svg.querySelector('.point'))$('detail').textContent='Measurements unavailable for this metric; inspect the checkpoint errors and CSV.';
}
$('metric').onchange=draw;draw();window.addEventListener('resize',draw);
</script></body></html>'''
