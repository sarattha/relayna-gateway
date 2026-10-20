#!/usr/bin/env python3
"""Build a self-contained interactive view from validated benchmark summaries."""

import argparse
import json
from pathlib import Path


TEMPLATE = r'''<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Gateway body admission benchmark</title><style>
*{box-sizing:border-box}body{margin:0;background:transparent;color:var(--foreground,#17212b);font-family:var(--font-sans,system-ui,sans-serif);font-size:14px;line-height:1.5}
:is(h2,p){margin:0}h2{font-size:20px;letter-spacing:-.03em}p{margin-top:8px}.muted{color:var(--muted-foreground,#667085);font-size:12px}
.legend{display:flex;gap:18px;margin:14px 0;font-size:12px}.swatch{display:inline-block;width:9px;height:9px;border-radius:2px;margin-right:6px}
.before{background:var(--chart-1,#2563eb)}.after{background:var(--chart-2,#0d9488)}
.charts{display:grid;grid-template-columns:1fr 1fr;gap:14px}.panel{padding:16px;border:1px solid var(--border,#dce2e8);border-radius:var(--radius,12px);background:var(--card,transparent)}
.panel h3{font-size:14px;font-weight:600;margin:0}.chart{display:block;width:100%;height:240px;overflow:visible}.chart text{fill:var(--muted-foreground,#667085);font-family:inherit;font-size:11px}
.controls{display:flex;align-items:center;flex-wrap:wrap;gap:8px;margin:20px 0 12px}.controls span{font-size:12px;margin-right:4px}.controls button{font:inherit;font-size:12px;cursor:pointer;border-radius:7px;border:1px solid var(--border,#dce2e8);background:transparent;color:inherit;padding:5px 12px}
.controls button[aria-pressed=true]{background:var(--accent-surface,#e2f3ef);color:var(--accent-surface-foreground,#0a6658);border-color:var(--ring,#0d9488)}button:focus-visible{outline:2px solid var(--ring,#0d9488);outline-offset:3px}
table{width:100%;border-collapse:collapse;font-size:12px;font-variant-numeric:tabular-nums}th,td{padding:9px 8px;border-bottom:1px solid var(--border,#dce2e8);text-align:right}th:first-child,td:first-child{text-align:left;padding-left:0}td:first-child{color:var(--muted-foreground,#667085)}thead th{font-weight:600}td:last-child,th:last-child{padding-right:0}
.note{margin-top:16px;font-size:12px;color:var(--muted-foreground,#667085)}
@media(max-width:600px){.charts{grid-template-columns:1fr}.panel{padding:16px}.chart{height:220px}h2{font-size:18px}table{font-size:11px}th,td{padding:8px 4px}}
</style></head><body>
<h2>Two-pod benchmark · 5 MiB requests</h2><p class="muted" id="method"></p>
<div class="legend"><span><i class="swatch before"></i>Baseline</span><span><i class="swatch after"></i>Improved</span></div>
<div class="charts"><section class="panel"><h3>Requests completed successfully</h3><svg id="success" class="chart" role="img" aria-label="Percentage of successful requests by body slot limit"></svg></section>
<section class="panel"><h3>Peak Gateway RSS · MiB per pod</h3><svg id="memory" class="chart" role="img" aria-label="Worst sampled resident memory of an individual gateway pod"></svg></section></div>
<div class="controls" id="controls" aria-label="Choose buffered body slots per pod"><span>Inspect slots per pod</span></div>
<table aria-live="polite"><thead><tr><th id="detail-title">Measurement</th><th>Baseline</th><th>Improved</th></tr></thead><tbody id="details"></tbody></table>
<p class="note">RSS is the worst sampled individual pod across three repetitions. Success timings exclude rejected requests and include the fixed mock delay. Buffered-slot peaks are sampled; kernel cgroup peaks are also retained.</p>
<p class="note">Local ARM64 kind cluster; direct pod traffic without ingress or TLS. The main matrix uses a 2 GiB limit per pod. Separate 1 GiB overload probes and full raw results are included in the repository report.</p>
<script>
const summaries = __DATA__;
const before=summaries[0].conditions,after=summaries[1].conditions;
const env=summaries[0].environment;
const limits=before.map(row=>row.limit);
const method=document.querySelector('#method');
method.textContent=`${env.rps} requests/s for ${env.duration_seconds}s · ${env.delay_ms/1000}s mock delay · ${env.repetitions} repetitions per limit · no retries`;
const ns='http://www.w3.org/2000/svg';
function node(name,attrs={},text){const result=document.createElementNS(ns,name);for(const [key,value] of Object.entries(attrs))result.setAttribute(key,value);if(text!==undefined)result.textContent=text;return result;}
function chart(id,key,percent=false){
 const svg=document.getElementById(id),width=svg.clientWidth,height=svg.clientHeight;
 svg.replaceChildren();svg.setAttribute('viewBox',`0 0 ${width} ${height}`);
 const left=40,right=7,top=22,bottom=30,plotW=width-left-right,plotH=height-top-bottom;
 const maxValue=percent?100:Math.max(...before.concat(after).map(row=>row[key]));
 const step=percent?25:Math.max(20,Math.ceil(maxValue*1.1/4/100)*100),max=percent?100:step*4;
 const color=['var(--chart-1,#2563eb)','var(--chart-2,#0d9488)'];
 for(let i=0;i<=4;i++){const value=max*i/4,y=top+plotH-value/max*plotH;svg.append(node('line',{x1:left,y1:y,x2:width-right,y2:y,stroke:'var(--border,#dce2e8)','stroke-width':1}));svg.append(node('text',{x:left-7,y:y+4,'text-anchor':'end'},`${Math.round(value)}${percent?'%':''}`));}
 for(let i=0;i<limits.length;i++){
  const center=left+(i+.5)*plotW/limits.length,barW=Math.min(20,plotW/limits.length*.29);
  [before,after].forEach((rows,j)=>{const value=rows[i][key],h=value/max*plotH;const rect=node('rect',{x:center+(j===0?-barW-1:1),y:top+plotH-h,width:barW,height:h,rx:2,fill:color[j],tabindex:0,'aria-label':`${j?'Improved':'Baseline'}, ${limits[i]} slots: ${value.toFixed(1)}${percent?' percent':' MiB'}`});rect.append(node('title',{},`${j?'Improved':'Baseline'} · ${limits[i]} slots · ${value.toFixed(1)}${percent?'%':' MiB'}`));svg.append(rect);});
  svg.append(node('text',{x:center,y:height-9,'text-anchor':'middle'},limits[i]));
 }
}
function fmt(value,digits=1){return value===null||value===undefined?'—':value.toLocaleString(undefined,{minimumFractionDigits:digits,maximumFractionDigits:digits});}
function inspect(limit){
 const rows=[before.find(row=>row.limit===limit),after.find(row=>row.limit===limit)];
 document.querySelectorAll('button').forEach(button=>button.setAttribute('aria-pressed',Number(button.dataset.limit)===limit));
 document.getElementById('detail-title').textContent=`${limit} slots per pod`;
 const fields=[
  ['200 responses',row=>`${row.successes} / ${row.requests} (${fmt(row.success_percent)}%)`],
  ['503 responses',row=>fmt(row.rejections,0)],
  ['Success latency p50 / p95, ms',row=>`${fmt(row.success_p50_ms)} / ${fmt(row.success_p95_ms)}`],
  ['Success TTFB p95, ms',row=>fmt(row.success_ttfb_p95_ms)],
  ['Rejection latency p95, ms',row=>fmt(row.rejection_p95_ms)],
  ['Peak RSS / cgroup, MiB per pod',row=>`${fmt(row.rss_peak_mib)} / ${fmt(row.cgroup_peak_mib)}`],
  ['Observed peak body slots',row=>fmt(row.buffered_requests_peak,0)],
  ['Restarts / client errors',row=>`${row.restarts} / ${row.unexpected_results}`]
 ];
 const tbody=document.getElementById('details');tbody.replaceChildren();
 for(const [label,read] of fields){const tr=document.createElement('tr');for(const value of [label,...rows.map(read)]){const td=document.createElement('td');td.textContent=value;tr.append(td);}tbody.append(tr);}
}
for(const limit of limits){const button=document.createElement('button');button.type='button';button.textContent=limit;button.dataset.limit=limit;button.addEventListener('click',()=>inspect(limit));document.getElementById('controls').append(button);}
const redraw=()=>{chart('success','success_percent',true);chart('memory','rss_peak_mib');};
new ResizeObserver(redraw).observe(document.body);redraw();inspect(limits[0]);
console.assert(before.length===after.length,'Comparison limit counts match');
console.assert(before.every((row,i)=>row.limit===after[i].limit),'Comparison limit values match');
console.log(JSON.stringify({comparisonRows:before.length+after.length,limits,requests:summaries.map(summary=>summary.conditions.reduce((sum,row)=>sum+row.requests,0))}));
</script></body></html>'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("summary")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    summaries = json.loads(Path(args.summary).read_text())
    if len(summaries) != 2:
        parser.error("expected baseline and improved summaries")
    encoded = json.dumps(summaries).replace("<", "\\u003c")
    Path(args.output).write_text(TEMPLATE.replace("__DATA__", encoded))


if __name__ == "__main__":
    main()
