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
const dispose = mountTraffic({content, initialMode:"live", api:async path=>{historyPath=path;return [historyRow];},headers:()=>({}),esc:String,attr:String,
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
const close=mountTraffic({initialMode:"live",content:{innerHTML:'',querySelector:selector=>node(selector.slice(1))},api:async path=>{calls.push(new URL(path,'http://test'));return pendingHistory?await pendingHistory:apiResult;},headers:()=>({}),esc:String,attr:String,table:(_,rows)=>{rendered=rows;return '';},badge:String,time:String,routingModeLabel:()=>'',onFilters:()=>{},investigationView:()=>''});
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
  await apply({});assert.equal(calls.at(-1).searchParams.get('time_basis'),'completed');
  assert.equal(Date.parse(calls.at(-1).searchParams.get('to'))-Date.parse(calls.at(-1).searchParams.get('from')),30*60*1000);
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

// Independent active snapshots retain quiet HTTP/SSE and WS records after journal
// churn, and stale progress cannot replace a completed row.
const quietSocket=make('quiet',{started_at:'2026-01-01T00:00:00Z',completed:false,diagnostics:{protocol:'websocket'},recording_failures:[]});
const quietHttp={...quietSocket,id:'quiet-http',diagnostics:{protocol:'http'}};
const snapshotMerge=mergeTrafficRows([make('terminal')],{gap:true,rows:[],active:{rows:[quietSocket,quietHttp]}},1);
assert.equal(snapshotMerge.length,2,'active rows use a separate budget from the completed journal');
assert.equal(matchesTraffic(quietSocket,{protocol:'websocket'}),true);
assert.equal(matchesTraffic(quietSocket,{protocol:'http'}),false);
assert.equal(matchesTraffic(quietHttp,{protocol:'http'}),true);
assert.equal(matchesTraffic({...quietSocket,completed:true},{outcome:'completed'}),true);
assert.equal(mergeTrafficRows([make('1')],{rows:[make('1',{completed:false})]})[0].completed,true);
console.log('ok - independent protocol partitions, quiet connections and terminal progress ordering');

const fullActive={...quietSocket,summary_only:false,stage:'upstream_connected',elapsed_ms:100,
  timeline:[{stage:'identity_verified',elapsed_ms:10}],upstream_timings:[{attempt:1,connect_ms:12}],
  usage:{total_tokens:0},debug_bundle:{routing_decisions:['service selected']},recording_failures:['history_write_failed'],
  diagnostics:{protocol:'websocket',websocket:{client_bytes:10,upstream_bytes:20},
    entra:{verification:'verified',source:'jwt',tenant_id:'synthetic-tenant'},
    authentication_profile:{id:'synthetic-profile',outcome:'verified'}}};
const activeSummary={...fullActive,summary_only:true,stage:'streaming',elapsed_ms:300,client_status:101,attempts:2,
  timeline:[],upstream_timings:[],usage:null,debug_bundle:null,recording_failures:[],
  diagnostics:{protocol:'websocket',websocket:{client_bytes:100,upstream_bytes:200},entra:null}};
const [investigated]=mergeTrafficRows([],{rows:[fullActive],active:{rows:[activeSummary]}});
assert.equal(investigated.summary_only,false);
for(const field of ['timeline','upstream_timings','usage','debug_bundle','recording_failures'])assert.deepEqual(investigated[field],fullActive[field],field);
for(const field of ['entra','authentication_profile'])assert.deepEqual(investigated.diagnostics[field],fullActive.diagnostics[field],field);
for(const field of ['stage','elapsed_ms','client_status','attempts'])assert.equal(investigated[field],activeSummary[field],field);
assert.deepEqual(investigated.diagnostics.websocket,activeSummary.diagnostics.websocket,'fresh summary counters update full investigations');
const [quietInvestigation]=mergeTrafficRows([investigated],{rows:[],active:{rows:[{...activeSummary,elapsed_ms:500}]}});
assert.deepEqual(quietInvestigation.timeline,fullActive.timeline,'quiet active snapshots retain previous full lifecycle details');
assert.equal(quietInvestigation.elapsed_ms,500);
for(const gap of [false,true]) {
  assert.equal(mergeTrafficRows([{...fullActive,completed:true}],{gap,rows:[fullActive],active:{rows:[activeSummary]}}).some(row=>!row.completed),false,'terminal rows cannot be resurrected by stale active updates');
  assert.equal(mergeTrafficRows([],{gap,rows:[{...fullActive,completed:true},fullActive],active:{rows:[activeSummary]}})[0].completed,true,'terminal updates win within the same batch');
}
console.log('ok - full active investigations survive summary refreshes with current counters and terminal ordering');

const clusterGlobals={document:globalThis.document,fetch:globalThis.fetch,FormData:globalThis.FormData,setTimeout:globalThis.setTimeout,clearTimeout:globalThis.clearTimeout};
const clusterNodes=new Map(), clusterControls=new Map(), clusterCalls=[],clusterTimers=new Map();
let clusterTimerId=-1;
let clusterRows=[], clusterResult={scope:'cluster',sampled_at:'2026-10-23T10:00:00Z',partial:false,unavailable:false,
  instances:[{instance_id:'pod-a',stale:false}],rows:[quietSocket,quietHttp],counts:{http:2000,websocket:10},matching:null,
  has_more:true,truncated:true,before:quietHttp.started_at,before_id:quietHttp.id};
let clusterPending=null;
const cn=id=>{
  if(!clusterNodes.has(id))clusterNodes.set(id,{textContent:'',innerHTML:'',value:'',handlers:{},disabled:false,
    classList:{toggle(){},add(){}},contains(){return false;},querySelectorAll(){return [];},
    elements:{namedItem(name){if(!clusterControls.has(name))clusterControls.set(name,{value:''});return clusterControls.get(name);}},
    addEventListener(name,handler){this.handlers[name]=handler;}});
  return clusterNodes.get(id);
};
globalThis.document={activeElement:null};
globalThis.fetch=async()=>{throw new Error('the default active view must use the cluster API');};
globalThis.FormData=class{constructor(){return new Map([...clusterControls].map(([name,c])=>[name,c.value]));}};
globalThis.setTimeout=(fn,ms,...args)=>{if(ms!==2000)return clusterGlobals.setTimeout(fn,ms,...args);const id=clusterTimerId--;clusterTimers.set(id,fn);return id;};
globalThis.clearTimeout=id=>{if(clusterTimers.has(id))clusterTimers.delete(id);else clusterGlobals.clearTimeout(id);};
const stopCluster=mountTraffic({content:{innerHTML:'',querySelector:selector=>cn(selector.slice(1))},
  api:async path=>{const url=new URL(path,'http://test');clusterCalls.push(url);return url.pathname.endsWith('/history')?[]:clusterPending?await clusterPending:clusterResult;},
  headers:()=>({}),esc:String,attr:String,table:(_,rows)=>{clusterRows=rows;return '';},badge:String,time:String,
  routingModeLabel:()=>'',investigationView:()=>''});
try {
  await flush();
  assert.equal(clusterCalls[0].pathname,'/admin-ui/admin/traffic/cluster');
  assert.equal(clusterCalls[0].searchParams.get('instance_id'),'all');
  assert.equal(clusterRows.length,2,'old quiet sockets and HTTP streams are discoverable');
  assert.match(cn('traffic-summary').textContent,/Exact active before filters: HTTP\/SSE 2000 · WebSocket 10/);
  assert.match(cn('traffic-warning').textContent,/saturated/);
  assert.equal(clusterTimers.size,1,'newest active page refreshes');
  const newestClusterResult=clusterResult;
  const olderActive={...quietSocket,id:'older-active',request_id:'older-active-page'};
  clusterResult={...clusterResult,rows:[olderActive],has_more:false,before:null,before_id:null};
  cn('traffic-older').handlers.click();await flush();
  assert.equal(clusterCalls.at(-1).searchParams.get('before'),quietHttp.started_at);
  assert.equal(clusterCalls.at(-1).searchParams.get('before_id'),quietHttp.id);
  assert.equal(clusterRows.length,1);assert.match(clusterRows[0][1],/older-active-page/);
  assert.equal(clusterTimers.size,0,'loaded older active pages remain frozen');
  const beforeActiveResume=clusterCalls.length;
  cn('traffic-pause').handlers.click();cn('traffic-pause').handlers.click();await flush();
  assert.equal(clusterCalls.length,beforeActiveResume,'Resume keeps the loaded older active page');
  assert.match(clusterRows[0][1],/older-active-page/);
  assert.equal(cn('traffic-older').disabled,true);assert.equal(cn('traffic-newest').disabled,false);
  assert.match(cn('traffic-connection').textContent,/older page remains frozen/);
  assert.equal(clusterTimers.size,0,'Resume does not restart polling for older active pages');
  clusterResult=newestClusterResult;
  cn('traffic-newest').handlers.click();await flush();assert.equal(clusterTimers.size,1);
  const displayedNewest=clusterRows;
  let resolveActiveOlder;clusterPending=new Promise(r=>{resolveActiveOlder=r;});
  cn('traffic-older').handlers.click();await flush();
  const activeOlderQuery=clusterCalls.at(-1).search;
  assert.deepEqual(clusterRows,displayedNewest,'loading Older retains the previous active page');
  assert.equal(cn('traffic-older').disabled,true);assert.equal(cn('traffic-newest').disabled,true);
  cn('traffic-pause').handlers.click();
  const beforeInterruptedResume=clusterCalls.length;
  cn('traffic-pause').handlers.click();await flush();
  assert.equal(clusterCalls.length,beforeInterruptedResume,'Resume does not jump to Newest after interrupting Older');
  assert.equal(cn('traffic-older').disabled,false);assert.equal(cn('traffic-newest').disabled,false);
  assert.equal(clusterTimers.size,0);
  resolveActiveOlder({...clusterResult,rows:[{...olderActive,request_id:'discarded-active-page'}]});clusterPending=null;await flush();
  assert.deepEqual(clusterRows,displayedNewest,'an interrupted Older response cannot overwrite the frozen page after Resume');
  clusterResult={...clusterResult,rows:[olderActive],has_more:false};
  cn('traffic-older').handlers.click();await flush();
  assert.equal(clusterCalls.at(-1).search,activeOlderQuery,'the retained cursor retries the interrupted active page');
  assert.match(clusterRows[0][1],/older-active-page/);assert.equal(clusterTimers.size,0);
  clusterResult=newestClusterResult;
  cn('traffic-instance').handlers.change({target:{value:'pod-a'}});await flush();
  assert.equal(clusterCalls.at(-1).searchParams.get('instance_id'),'pod-a');
  clusterControls.set('protocol',{value:'websocket'});
  clusterControls.set('from',{value:'2026-10-23T09:00'});
  cn('traffic-filters').handlers.submit({preventDefault(){},currentTarget:cn('traffic-filters')});await flush();
  assert.equal(clusterCalls.at(-1).searchParams.get('protocol'),'websocket');
  assert.equal(clusterCalls.at(-1).searchParams.has('from'),false,'completed-time filters must not hide old active connections');
  assert.equal(clusterRows.length,1);
  clusterResult={...clusterResult,partial:true,instances:[{instance_id:'pod-a',stale:true}],counts:{http:0,websocket:0}};
  cn('traffic-newest').handlers.click();await flush();
  assert.match(cn('traffic-summary').textContent,/Known active/);
  assert.match(cn('traffic-warning').textContent,/rows may be stale/);
  clusterResult={...clusterResult,unavailable:true};
  cn('traffic-newest').handlers.click();await flush();
  assert.match(cn('traffic-summary').textContent,/Current counts unknown/);
  assert.match(cn('traffic-warning').textContent,/coverage are unknown/);
  let resolve;clusterPending=new Promise(r=>{resolve=r;});
  cn('traffic-newest').handlers.click();await flush();
  cn('traffic-mode').handlers.change({target:{value:'history'}});await flush();
  assert.equal(clusterRows.length,0);
  resolve({...clusterResult,rows:[quietSocket]});await flush();
  assert.equal(clusterRows.length,0,'late cluster responses cannot overwrite completed history');
  assert.equal(clusterCalls.at(-1).searchParams.get('time_basis'),'completed');
  assert.equal(clusterCalls.at(-1).searchParams.get('protocol'),'websocket');
} finally {stopCluster();Object.assign(globalThis,clusterGlobals);}
console.log('ok - default cluster active view, exact counts, stale coverage, server filters, frozen active pages across Pause/Resume, interrupted Older and mode races');

// The newest completion page refreshes on one bounded five-second timer. Keep
// its rows during a pending refresh; historical pages and explicit end times freeze.
const historyGlobals={document:globalThis.document,fetch:globalThis.fetch,FormData:globalThis.FormData,setTimeout:globalThis.setTimeout,clearTimeout:globalThis.clearTimeout};
const historyNodes=new Map(),historyControls=new Map(),historyCalls=[],historyTimers=new Map();
let timerId=-1,historyRendered=[],historyHeaders=[],historyPending=null;
const completionRows=Array.from({length:100},(_,i)=>make(`completed-${i}`,{started_at:'2026-01-01T00:00:00Z',completed_at:'2026-10-23T10:00:00Z',recording_failures:[]}));
let historyResult=completionRows;
const hn=id=>{
  if(!historyNodes.has(id))historyNodes.set(id,{textContent:'',innerHTML:'',value:'',handlers:{},disabled:false,
    classList:{toggle(){},add(){}},contains(){return false;},querySelectorAll(){return [];},
    elements:{namedItem(name){if(!historyControls.has(name))historyControls.set(name,{value:''});return historyControls.get(name);}},
    addEventListener(name,handler){this.handlers[name]=handler;}});
  return historyNodes.get(id);
};
globalThis.document={activeElement:null};
globalThis.fetch=async()=>{throw new Error('saved history must not open SSE');};
globalThis.FormData=class{constructor(){return new Map([...historyControls].map(([name,c])=>[name,c.value]));}};
globalThis.setTimeout=(fn,ms,...args)=>{if(ms!==5000)return historyGlobals.setTimeout(fn,ms,...args);const id=timerId--;historyTimers.set(id,fn);return id;};
globalThis.clearTimeout=id=>{if(historyTimers.has(id))historyTimers.delete(id);else historyGlobals.clearTimeout(id);};
const historyDispose=mountTraffic({initialMode:'history',content:{innerHTML:'',querySelector:selector=>hn(selector.slice(1))},
  api:async path=>{historyCalls.push(new URL(path,'http://test'));return historyPending?await historyPending:historyResult;},
  headers:()=>({}),esc:String,attr:String,table:(headers,rows)=>{historyHeaders=headers;historyRendered=rows;return '';},badge:String,time:String,routingModeLabel:()=>'',investigationView:()=>''});
try {
  await flush();assert.equal(historyTimers.size,1);assert.equal(historyRendered.length,100);
  const [id,poll]=[...historyTimers][0];historyTimers.delete(id);
  let resolve;historyPending=new Promise(r=>{resolve=r;});
  const polling=poll();await flush();
  assert.equal(historyRendered.length,100,'polling keeps the displayed page while awaiting the DB');
  resolve([completionRows[0]]);await polling;historyPending=null;await flush();
  assert.equal(historyRendered.length,1);assert.equal(historyTimers.size,1);
  hn('traffic-newest').handlers.click();await flush();assert.equal(historyRendered.length,100);
  let resolveOlder;historyPending=new Promise(r=>{resolveOlder=r;});
  hn('traffic-older').handlers.click();await flush();
  assert.equal(historyTimers.size,0,'older pages remain frozen');
  assert.equal(historyCalls.at(-1).searchParams.get('before'),completionRows.at(-1).completed_at);
  const olderQuery=historyCalls.at(-1).search;
  assert.equal(historyRendered.length,100,'loading Older retains the usable previous page');
  assert.equal(hn('traffic-older').disabled,true);assert.equal(hn('traffic-newest').disabled,true);
  hn('traffic-pause').handlers.click();
  resolveOlder([{...completionRows[0],request_id:'discarded-older-response'}]);historyPending=null;await flush();
  assert.equal(historyRendered.length,100,'a paused Older response cannot overwrite the previous page');
  const beforeResume=historyCalls.length;
  hn('traffic-pause').handlers.click();await flush();
  assert.equal(historyCalls.length,beforeResume,'resuming an older page must not jump to Newest');
  assert.equal(hn('traffic-older').disabled,false);assert.equal(hn('traffic-newest').disabled,false);
  assert.equal(historyTimers.size,0);
  historyResult=[{...completionRows[0],request_id:'recovered-older-page'}];
  hn('traffic-older').handlers.click();await flush();
  assert.equal(historyCalls.at(-1).search,olderQuery,'the restored cursor retries the interrupted Older page');
  assert.equal(historyRendered.length,1);assert.match(historyRendered[0][1],/recovered-older-page/);
  historyResult=completionRows;
  hn('traffic-newest').handlers.click();await flush();assert.equal(historyTimers.size,1);
  hn('traffic-pause').handlers.click();assert.equal(historyTimers.size,0,'Pause stops history refresh');
  hn('traffic-pause').handlers.click();await flush();assert.equal(historyTimers.size,1,'Resume restarts the newest page');
  historyControls.set('to',{value:'2026-09-03T11:00'});
  hn('traffic-filters').handlers.submit({preventDefault(){},currentTarget:hn('traffic-filters')});await flush();
  assert.equal(historyTimers.size,0,'an explicit fixed end time remains manual');
  historyResult=[{...completionRows[0],completed_at:null}];
  hn('traffic-mode').handlers.change({target:{value:'legacy'}});await flush();
  const legacyQuery=historyCalls.at(-1).searchParams;
  assert.equal(legacyQuery.get('time_basis'),'started');
  assert.equal(Date.parse(legacyQuery.get('to'))-Date.parse(legacyQuery.get('from')),24*60*60*1000);
  assert.equal(historyRendered.length,1,'the UI retains access to legacy rows without completion timestamps');
  assert.equal(historyHeaders[0],'Arrived');assert.equal(historyTimers.size,0);
} finally {historyDispose();Object.assign(globalThis,historyGlobals);}
console.log('ok - newest completion polling, stable pending rows, frozen older pages, Pause/Resume and legacy arrival history');
