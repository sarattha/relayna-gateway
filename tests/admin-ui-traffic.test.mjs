import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";
const source = readFileSync(new URL("../crates/gateway-api/admin-ui/src/traffic.ts", import.meta.url), "utf8");
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 } }).outputText;
const { parseTrafficFrames, mergeTrafficRows, matchesTraffic } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString("base64")}`);
const make = (id, overrides = {}) => ({ id, request_id: "same-client-id", started_at: `2026-09-03T00:00:0${id}Z`, completed: true, client_status: 200, diagnostics: {}, ...overrides });
const streamFailure = make("1", { streaming: true, diagnostics: { failure_code: "upstream_connection_closed", outcome: "stream_interrupted" } });
assert.equal(matchesTraffic(streamFailure, { outcome: "failures" }), true);
assert.equal(matchesTraffic(streamFailure, { outcome: "active" }), false);
assert.equal(matchesTraffic(streamFailure, { status: "503" }), false);
const event = `event: traffic\nid: instance:1\ndata: ${JSON.stringify({ cursor: "instance:1", rows: [streamFailure] })}\n\n`;
for (let split = 0; split < event.length; split++) {
  const first = parseTrafficFrames(event.slice(0, split));
  const second = parseTrafficFrames(first.remainder + event.slice(split));
  assert.equal([...first.batches, ...second.batches].length, 1);
  assert.equal(second.remainder, "");
}
assert.equal(parseTrafficFrames(event.replaceAll("\n", "\r\n")).batches[0].rows[0].client_status, 200);
const merged = mergeTrafficRows([make("1")], { rows: [make("1", { client_status: 503 }), make("2")] });
assert.equal(merged.length, 2, "distinct internal IDs survive reused client IDs");
assert.equal(merged.find((row) => row.id === "1").client_status, 503);
assert.equal(mergeTrafficRows(merged, { gap: true, rows: [make("3")] }).length, 1, "gaps clear stale active state");
assert.equal(mergeTrafficRows(merged, { rows: [make("3")] }, 2).length, 2);
console.log("ok - traffic SSE split frames, stream outcomes, bounded merging, replay gaps and reused IDs");

// Exercise the mounted event handlers with a delayed live reader, not just the
// pure predicate: aborted fetches can still resolve an already-buffered read.
const { mountTraffic } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString("base64")}`);
const originals = { document: globalThis.document, fetch: globalThis.fetch, FormData: globalThis.FormData };
const elements = new Map();
const fieldValues = new Map();
function element(id) {
  if (!elements.has(id)) elements.set(id, {
    textContent: '', innerHTML: '', value: '', handlers: {},
    classList: { toggle() {}, add() {} },
    elements: { namedItem(name) { if (!fieldValues.has(name)) fieldValues.set(name, {value:''}); return fieldValues.get(name); } },
    addEventListener(name, fn) { this.handlers[name] = fn; },
    querySelectorAll() { return []; }, contains() { return false; },
  });
  return elements.get(id);
}
let finishRead;
const pendingRead = new Promise(resolve => { finishRead = resolve; });
let displayed = [];
let applied;
let historyPath;
const content = { innerHTML: '', querySelector: selector => element(selector.slice(1)) };
globalThis.document = { activeElement: null };
globalThis.FormData = class { constructor() { return new Map([...fieldValues].map(([name, field]) => [name, field.value])); } };
globalThis.fetch = async () => ({ok:true,body:{getReader:()=>({read:()=>pendingRead,cancel:async()=>{}})}});
const historyRow = make('5', {request_id:'history',recording_failures:[],elapsed_ms:1});
const dispose = mountTraffic({content, api:async path=>{historyPath=path;return [historyRow];},headers:()=>({}),esc:String,attr:String,
  table:(_,rows)=>{displayed=rows;return '';},badge:String,time:String,routingModeLabel:()=>'',
  onFilters:filters=>{applied=filters;},investigationView:()=>''});
try {
  fieldValues.set('request_id',{value:' history '});
  element('traffic-filters').handlers.submit({preventDefault(){},currentTarget:element('traffic-filters')});
  assert.equal(applied.request_id,'history','trim pasted filter values before applying');
  element('traffic-mode').handlers.change({target:{value:'history'}});
  await new Promise(resolve=>setImmediate(resolve));
  assert.match(historyPath,/request_id=history/);
  assert.equal(displayed.length,1);
  finishRead({done:false,value:new TextEncoder().encode(`data: ${JSON.stringify({instance_id:'stale',cursor:'stale:1',gap:true,rows:[make('6',{request_id:'live',recording_failures:[]})]})}\n\n`)});
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(displayed.length,1,'late live batch must not replace filtered saved history');
  assert.match(displayed[0][1],/history/);
  assert.match(element('traffic-connection').textContent,/Saved history/);
} finally {
  dispose();
  Object.assign(globalThis,originals);
}
console.log('ok - mounted Traffic trims applied filters and ignores stale live reads after switching to history');

// Every selectable live predicate, including intersected filters and missing metadata.
const p='aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', k='bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb';
const base=make('7',{request_id:'prefix-correlation-suffix',service:'orders',project_id:p,key_id:k,client_status:503,diagnostics:{failure_code:'upstream_timeout'},recording_failures:[]});
const fields={request_id:'correlation',service:'orders',project_id:p,key_id:k,status:'503',failure_code:'upstream_timeout'};
const mismatches={request_id:'another',service:'other',project_id:k,key_id:p,status:'200',failure_code:'another_failure'};
for(const [name,value] of Object.entries(fields)) {
  assert.equal(matchesTraffic(base,{[name]:value}),true,name);
  assert.equal(matchesTraffic(base,{[name]:mismatches[name]}),false,name);
  assert.equal(matchesTraffic(base,{[name]:''}),true,`${name} blank`);
}
for(let mask=0;mask<64;mask++) {
  const filters=Object.fromEntries(Object.entries(fields).filter((_,i)=>mask&(1<<i)));
  for(const outcome of ['all','failures','active']) assert.equal(matchesTraffic(base,{...filters,outcome}),outcome!=='active');
  for(const name of Object.keys(filters)) assert.equal(matchesTraffic(base,{...filters,[name]:mismatches[name]}),false,`${mask} ${name}`);
}
for(const status of [100,200,204,302,400,401,403,404,429,500,502,503,599]) assert.equal(matchesTraffic({...base,client_status:status},{status:String(status)}),true);
const absent={...base,id:"8",service:null,project_id:null,key_id:null,client_status:null,diagnostics:{}};
for(const name of ['service','project_id','key_id','status','failure_code'])assert.equal(matchesTraffic(absent,{[name]:fields[name]}),false);
assert.equal(matchesTraffic({...base,completed:false},{outcome:'active'}),true);
assert.equal(matchesTraffic(absent,{outcome:'failures'}),false);
assert.equal(matchesTraffic(streamFailure,{outcome:'failures',status:'200'}),true,'stream interruption remains a failure after HTTP 200');
console.log('ok - all live Traffic filters, 192 outcome/intersection combinations, HTTP status classes and missing fields');

// Exercise the actual mounted form, history query builder, chips and pagination.
const savedGlobals={document:globalThis.document,fetch:globalThis.fetch,FormData:globalThis.FormData};
const controls=new Map(), nodes=new Map(), calls=[];
let rendered=[], apiResult=[], pendingHistory;
const node=id=>{
  if(!nodes.has(id))nodes.set(id,{
    value:'',textContent:'',handlers:{},disabled:false,html:'',buttons:[],classList:{toggle(){},add(){}},
    elements:{namedItem(name){if(!controls.has(name))controls.set(name,{value:'',focus(){}});return controls.get(name);}},
    addEventListener(name,handler){this.handlers[name]=handler;},contains(){return false;},
    querySelectorAll(){return this.buttons;},
    set innerHTML(value){this.html=value;this.buttons=id==='traffic-failure-groups'?[...value.matchAll(/data-traffic-reason="([^"]+)"/g)].map(match=>({dataset:{trafficReason:match[1]},handlers:{},addEventListener(name,handler){this.handlers[name]=handler;},focus(){}})):[];},
    get innerHTML(){return this.html;},
  });return nodes.get(id);
};
const flush=()=>new Promise(resolve=>setImmediate(resolve));
let reads=[];let wake;
const emit=async rows=>{const value={done:false,value:new TextEncoder().encode(`data: ${JSON.stringify({instance_id:'matrix',cursor:'matrix:1',rows})}\n\n`)};if(wake){const f=wake;wake=null;f(value);}else reads.push(value);await flush();};
globalThis.document={activeElement:null};
globalThis.FormData=class{constructor(){return new Map([...controls].map(([name,c])=>[name,c.value]));}};
globalThis.fetch=async(_url,{signal})=>{
  const reader={finish:null,read(){return reads.length?Promise.resolve(reads.shift()):new Promise(resolve=>{this.finish=resolve;wake=resolve;});},async cancel(){this.finish?.({done:true});if(wake===this.finish)wake=null;}};
  signal.addEventListener('abort',()=>reader.cancel(),{once:true});
  return {ok:true,body:{getReader:()=>reader}};
};
const close=mountTraffic({content:{innerHTML:'',querySelector:selector=>node(selector.slice(1))},api:async path=>{calls.push(new URL(path,'http://test'));return pendingHistory?await pendingHistory:apiResult;},headers:()=>({}),esc:String,attr:String,table:(_,rows)=>{rendered=rows;return '';},badge:String,time:String,routingModeLabel:()=>'',onFilters:()=>{},investigationView:()=>''});
const apply=async values=>{controls.clear();for(const [name,value]of Object.entries(values))controls.set(name,{value});node('traffic-filters').handlers.submit({preventDefault(){},currentTarget:node('traffic-filters')});await flush();};
const mode=async value=>{node('traffic-mode').handlers.change({target:{value}});await flush();};
try {
  await flush();await emit([base,absent]);
  await apply({project_id:` ${p.toUpperCase()} `,key_id:` ${k.toUpperCase()} `,status:'0503'});
  assert.equal(rendered.length,1,'UUID case and numeric status normalize in live mode');
  assert.equal(controls.get('project_id').value,p);assert.equal(controls.get('status').value,'503');
  apiResult=[base];await mode('history');
  for(const [name,value] of Object.entries(fields)) {
    await apply({[name]:` ${value} `});
    assert.equal(calls.at(-1).searchParams.get(name),value,`${name} query`);
    for(const other of Object.keys(fields).filter(other=>other!==name))assert.equal(calls.at(-1).searchParams.has(other),false);
  }
  const from='2026-09-03T10:00',to='2026-09-03T11:00';
  await apply({...fields,outcome:'failures',from,to});
  const query=calls.at(-1).searchParams;
  assert.equal(query.get('failures_only'),'true');assert.equal(query.get('from'),new Date(from).toISOString());assert.equal(query.get('to'),new Date(to).toISOString());
  assert.equal(rendered.length,1);
  await apply({outcome:'all'});assert.equal(calls.at(-1).searchParams.has('failures_only'),false);
  const callCount=calls.length;await apply({outcome:'active'});assert.equal(calls.length,callCount);assert.match(node('traffic-warning').textContent,/Active requests.*Live/);
  await apply({from:'invalid-date'});assert.match(node('traffic-warning').textContent,/History unavailable/);assert.equal(node('traffic-older').disabled,true);
  await apply({});assert.equal(calls.at(-1).searchParams.toString(),'limit=100');
  // Reason chips apply and toggle off an exact reason without changing other fields.
  node('traffic-failure-groups').buttons[0].handlers.click();await flush();assert.equal(calls.at(-1).searchParams.get('failure_code'),'upstream_timeout');
  node('traffic-failure-groups').buttons[0].handlers.click();await flush();assert.equal(calls.at(-1).searchParams.has('failure_code'),false);
  apiResult=Array.from({length:100},(_,i)=>({...base,id:`row-${i}`}));await apply({service:'orders'});
  assert.equal(node('traffic-older').disabled,false);
  node('traffic-older').handlers.click();await flush();assert.equal(calls.at(-1).searchParams.get('before_id'),'row-99');
  node('traffic-newest').handlers.click();await flush();assert.equal(calls.at(-1).searchParams.has('before_id'),false);
  let resolve;pendingHistory=new Promise(r=>{resolve=r;});await apply({service:'new-service'});
  assert.equal(node('traffic-older').disabled,true);assert.equal(node('traffic-newest').disabled,true);
  const beforeCalls=calls.length;node('traffic-older').handlers.click();await flush();assert.equal(calls.length,beforeCalls,'old cursor cannot page a new filter while loading');
  resolve([]);pendingHistory=null;await flush();assert.equal(node('traffic-newest').disabled,false);assert.equal(node('traffic-older').disabled,true);assert.equal(rendered.length,0);
  await mode('live');await apply({outcome:'active'});await emit([{...base,completed:false}]);assert.equal(rendered.length,1);
  node('traffic-pause').handlers.click();assert.match(node('traffic-connection').textContent,/Paused/);
  node('traffic-pause').handlers.click();await flush();await emit([{...base,completed:true}]);assert.equal(rendered.length,0,'completion removes row from Active only');
}finally{close();Object.assign(globalThis,savedGlobals);}
console.log('ok - all mounted history filters, normalization, dates, chips, pagination reset, empty results, pause/resume and active completion');
