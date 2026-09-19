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
