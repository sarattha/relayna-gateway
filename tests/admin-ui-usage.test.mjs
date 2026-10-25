import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";
import ts from "typescript";

const visualsSource = readFileSync(new URL("../crates/gateway-api/admin-ui/src/monitoring-visuals.ts", import.meta.url), "utf8");
const visuals = await import(`data:text/javascript;base64,${Buffer.from(visualsSource).toString("base64")}`);
const source = readFileSync(new URL("../crates/gateway-api/admin-ui/src/main.ts", import.meta.url), "utf8");
const ast = ts.createSourceFile("main.ts", source, ts.ScriptTarget.Latest, true);
const extract = name => {
  const node = ast.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === name);
  assert.ok(node, name);
  return node.getText(ast);
};
const tick = () => new Promise(resolve => setTimeout(resolve, 0));
const deferred = () => { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; };

function harness(respond) {
  const dom = new JSDOM('<main id="content"></main>');
  const state = { projects: [], keys: [], services: [], projectScope: "", usageFilters: {}, usageTab: "Recent requests", usagePagination: { eventsOffset: 0, timeseriesOffset: 0, serviceTimeseriesOffset: 0 } };
  const calls = [];
  const names = ["usage", "loadUsage", "usageRequestCurrent", "usageRequestController", "loadUsageSummary", "loadUsageVisuals", "loadVisibleUsageSections", "readUsage", "drainUsageReads", "loadUsageSection", "bindUsageSuggestions", "usageIdentifierLabels", "usageProjectName", "usageKeyName", "usageQueryFromForm", "usageFilterValuesQueryFromForm", "usageDateRange", "usagePageSize", "usagePaginationKey", "changeUsagePage", "usagePagedTable", "tabulateUsage"];
  const api = async (path, options = {}) => {
    calls.push({ path, options });
    if (["projects", "keys", "services"].some(name => path === `/admin-ui/admin/${name}`)) return [];
    return respond(path, options);
  };
  const ui = new Function("document", "FormData", "Option", "state", "api", "visuals", `
    const content = document.querySelector('#content');
    const { usageChartsShell, usageSummaryView, usageTrendConfigs, mountMonitorChart, usageRanking } = visuals;
    const Chart = null;
    let renderGeneration=0, usageGeneration=0, usageLoad=null, usageLabelCache=null;
    const esc=x=>String(x), attr=esc, money=esc, percent=esc, stat=(name,value)=>name+': '+value;
    const emptyState=esc, formSection=(_title,_hint,markup)=>markup;
    const projectOptions=()=>'', keyOptions=()=>'', serviceOptions=()=>'';
    const handleAsync=fn=>fn, organizeView=()=>{}, syncUsageExportControls=()=>{};
    const showContentDrawer=(title,markup)=>{const drawer=document.createElement('div');drawer.dataset.chartData='true';drawer.innerHTML=title+markup;document.body.appendChild(drawer);};
    const applyUsageFilters=()=>{}, loadTaskUsage=()=>{}, usageExportAction=()=>{}, showLiteLlmSpend=()=>{}, openDebugRequest=()=>{};
    const usageEventsTable=rows=>'<div data-events>'+rows.map(row=>row.request_id).join(',')+'</div>';
    const usageBreakdownTable=rows=>JSON.stringify(rows), usageTimeseriesTable=usageBreakdownTable, usageServiceTimeseriesTable=usageBreakdownTable;
    const usageProjectKeyServiceHierarchy=usageBreakdownTable, unusedKeysTable=usageBreakdownTable;
    ${names.map(extract).join("\n")}
    return {usage,loadUsage,loadUsageSummary,loadUsageSection,usageQueryFromForm,usageIdentifierLabels,usageProjectName,usageKeyName};
  `)(dom.window.document, dom.window.FormData, dom.window.Option, state, api, visuals);
  return { dom, document: dom.window.document, state, calls, ui };
}

{
  const summary = deferred();
  const h = harness(path => {
    if (path.includes('/summary?')) return summary.promise;
    if (path.includes('/events?')) return { rows: [{ request_id: 'first-page' }], has_more: true, next_cursor: 'second-cursor', snapshot_cursor: 'first-snapshot', limit: 50 };
    return { rows: [] };
  });
  const initial = h.ui.usage();
  await tick();
  assert.match(h.document.querySelector('[data-events]').textContent, /first-page/);
  assert.match(h.document.querySelector('[data-usage-summary]').textContent, /Loading/);
  assert.equal(h.document.querySelector('[name=time_preset]').value, 'last_24h');
  const dateQuery = h.ui.usageQueryFromForm();
  assert.ok(dateQuery.get('from') && dateQuery.get('to'));
  assert.equal(h.calls.filter(call => /sections\/timeseries/.test(call.path)).length, 1, 'one bounded optional chart read');
  assert.equal(h.calls.filter(call => /sections\/(?!timeseries)|filter-values/.test(call.path)).length, 0);
  summary.reject(new Error('summary timed out'));
  await initial;
  assert.match(h.document.querySelector('[data-usage-summary]').textContent, /summary unavailable/);
  assert.match(h.document.querySelector('[data-events]').textContent, /first-page/);
  h.document.querySelector('[data-usage-page-direction=next]').click();
  await tick();
  const events = h.calls.filter(call => call.path.includes('/events?'));
  assert.equal(events.length, 2);
  assert.equal(new URL(events[1].path, 'http://test').searchParams.get('cursor'), 'second-cursor');
  assert.equal(new URL(events[1].path, 'http://test').searchParams.get('to'), new URL(events[0].path, 'http://test').searchParams.get('to'));
  assert.equal(h.calls.filter(call => call.path.includes('/summary?')).length, 1, 'section pagination must not recompute summary');
  h.document.querySelector('[data-usage-page-direction=previous]').click();
  await tick();
  const previous = h.calls.filter(call => call.path.includes('/events?')).at(-1);
  assert.equal(new URL(previous.path, 'http://test').searchParams.get('cursor'), 'first-snapshot', 'Previous must keep the original insertion boundary');
  [...h.document.querySelectorAll('.result-tabs button')].find(button => button.textContent === 'Projects').click();
  await tick();
  assert.equal(h.calls.filter(call => call.path.includes('/sections/projects?')).length, 1);
  assert.equal(h.calls.filter(call => call.path.includes('/sections/models?')).length, 0);
  h.document.querySelector('[name=time_preset]').value = 'all_time';
  assert.equal(h.ui.usageQueryFromForm().has('from'), false);
  assert.equal(h.ui.usageQueryFromForm().has('to'), false);
  h.dom.window.close();
}
console.log('ok - Usage defaults to 24h, settles summary/events independently, loads sections lazily and pages one frozen query');

{
  const rows = Array.from({length: 200}, (_, index) => ({request_id: `request-${index}`}));
  const h = harness(path => {
    if (path.includes('/summary?')) return {request_count: rows.length};
    const params = new URL(path, 'http://test').searchParams;
    const section = path.includes('/events?') ? 'events' : path.match(/\/sections\/([^?]+)/)?.[1];
    const offsetKey = section === 'timeseries' ? 'timeseries_offset' : section === 'service-timeseries' ? 'service_timeseries_offset' : 'offset';
    const limitKey = section === 'timeseries' ? 'timeseries_limit' : section === 'service-timeseries' ? 'service_timeseries_limit' : 'limit';
    const offset = Number(params.get(offsetKey) || 0), limit = Number(params.get(limitKey) || 50);
    const page = {limit,offset,has_more:offset + limit < rows.length};
    return {rows:rows.slice(offset, offset + limit),...page,page};
  });
  h.state.usageFilters = {sort_by:'cost',limit:'50'};
  await h.ui.usage();
  const initial = h.document.querySelector('[data-events]').textContent.split(',');
  // Editing the pending form must not change any applied page's stride.
  h.document.querySelector('#usage-form [name=limit]').value = '10';
  h.document.querySelector('[data-usage-page-section=events][data-usage-page-direction=next]').click();
  await tick();
  const next = h.document.querySelector('[data-events]').textContent.split(',');
  assert.equal(initial.length, 50);
  assert.equal(next.length, 50);
  assert.equal(next[0], 'request-50');
  assert.equal(new Set([...initial, ...next]).size, 100, 'pages must not overlap after an unapplied limit edit');
  assert.match(h.document.querySelector('[data-usage-section=events]').textContent, /Showing 51-100/);
  h.document.querySelector('[data-usage-page-section=events][data-usage-page-direction=previous]').click();
  await tick();
  assert.equal(h.document.querySelector('[data-events]').textContent.split(',')[0], 'request-0');
  for (const [section, tab, offsetKey, limitKey] of [
    ['timeseries', 'Timeseries', 'timeseries_offset', 'timeseries_limit'],
    ['service-timeseries', 'Service timeseries', 'service_timeseries_offset', 'service_timeseries_limit'],
  ]) {
    [...h.document.querySelectorAll('.result-tabs button')].find(button => button.textContent === tab).click();
    await tick();
    h.document.querySelector(`[data-usage-page-section=${section}][data-usage-page-direction=next]`).click();
    await tick();
    const call = h.calls.filter(call => call.path.includes(`/sections/${section}?`)).at(-1);
    const params = new URL(call.path, 'http://test').searchParams;
    assert.equal(params.get(offsetKey), '50');
    assert.equal(params.get(limitKey), '50');
    assert.match(h.document.querySelector(`[data-usage-section=${section}]`).textContent, /Showing 51-100/);
  }
  h.dom.window.close();
}
console.log('ok - unapplied page-size edits keep events and both timeseries pagers on the frozen applied limit without overlap');

{
  let fail = true;
  const h = harness(path => {
    if (path.includes('/summary?')) return { request_count: 17 };
    if (path.includes('/events?') && fail) throw new Error('rows timed out');
    return { rows: [{ request_id:'recovered' }], has_more:false };
  });
  await h.ui.usage();
  assert.equal(h.document.querySelector('[data-usage-summary] .monitor-kpi strong').textContent, '17');
  assert.match(h.document.querySelector('[data-usage-section=events]').textContent,/unavailable/);
  fail = false;
  h.document.querySelector('[data-retry-usage-section]').click();
  await tick();
  assert.match(h.document.querySelector('[data-events]').textContent,/recovered/);
  assert.equal(h.calls.filter(call => call.path.includes('/summary?')).length,1);
  h.dom.window.close();
}
console.log('ok - failed event reads retain the independent summary and recover without recomputing it');

{
  let chartsFail = true;
  const h = harness(path => {
    if (path.includes('/summary?')) return { request_count: 17 };
    if (path.includes('/sections/timeseries?')) {
      if (chartsFail) throw new Error('chart read unavailable');
      return { rows: [{ bucket: '2026-10-23T00:00:00Z', summary: { request_count: 17, success_count: 16, failure_count: 1, estimated_cost_usd: 0 } }] };
    }
    return { rows: [{ request_id: 'independent-events' }], has_more: false };
  });
  await h.ui.usage();
  await tick();
  assert.equal(h.document.querySelector('[data-usage-summary] .monitor-kpi strong').textContent, '17');
  assert.match(h.document.querySelector('[data-events]').textContent, /independent-events/);
  assert.match(h.document.querySelector('[data-usage-visuals]').textContent, /Charts unavailable/);
  chartsFail = false;
  h.document.querySelector('[data-retry-usage-charts]').click();
  await tick();
  assert.equal(h.document.querySelectorAll('[data-usage-visuals] canvas').length, 2);
  const chartQuery = new URL(h.calls.find(call => call.path.includes('/sections/timeseries?')).path, 'http://test').searchParams;
  assert.equal(chartQuery.get('timeseries_limit'), '168');
  assert.equal(chartQuery.get('timeseries_offset'), '0');
  assert.ok(chartQuery.get('from') && chartQuery.get('to'));
  h.document.querySelector('[data-usage-chart-data]').click();
  await tick();
  assert.match(h.document.querySelector('[data-chart-data]').textContent, /Chart data · hourly buckets/);
  assert.match(h.document.querySelector('[data-chart-data]').textContent, /2026-10-23T00:00:00Z/);
  assert.equal(h.calls.filter(call => call.path.includes('/sections/timeseries?')).length, 2, 'the Data drawer reuses the exact chart response without another read');
  assert.equal(h.calls.filter(call => call.path.includes('/summary?')).length, 1);
  h.dom.window.close();
}
console.log('ok - chart failures preserve Usage results, bounded chart retries recover, and chart Data opens the source table');

{
  const pending = [];
  let active = 0, maximum = 0;
  const h = harness(path => {
    if (!path.includes('/sections/')) return path.includes('/summary?') ? {request_count:0} : {rows:[],has_more:false};
    active += 1; maximum = Math.max(maximum,active);
    return new Promise(resolve => pending.push(() => { active -= 1; resolve({rows:[],page:{limit:50,offset:0,has_more:false}}); }));
  });
  h.state.projectScope = 'saved-project';
  h.state.usageFilters = {key_id:'saved-key',time_preset:'all_time'};
  await h.ui.usage();
  assert.equal(h.document.querySelector('[name=project_id]').value,'saved-project');
  assert.equal(h.document.querySelector('[name=key_id]').value,'saved-key');
  assert.equal(h.ui.usageQueryFromForm().get('key_id'),'saved-key');
  assert.equal(h.ui.usageQueryFromForm().has('from'),false);
  [...h.document.querySelectorAll('.result-tabs button')].find(button => button.textContent === 'Recent requests').click();
  assert.equal(pending.length,3);
  while (pending.length) { pending.shift()(); await tick(); }
  assert.equal(maximum,3);
  assert.equal(h.calls.filter(call => call.path.includes('/sections/')).length,11);
  h.dom.window.close();
}
console.log('ok - explicit all time and saved project/key filters survive progressive inventories; Show all bounds section concurrency');

{
  const flights = new Set();
  let maximum = 0, aborted = 0;
  const h = harness((path, {signal}) => new Promise((resolve, reject) => {
    const flight = { path, finish(error) {
      if (!flights.delete(flight)) return;
      signal.removeEventListener('abort', cancel);
      if (error) reject(error);
      else resolve(path.includes('/summary?') ? {request_count:0} : {rows:[],has_more:false,limit:50,page:{limit:50,offset:0,has_more:false}});
    } };
    const cancel = () => { aborted += 1; flight.finish(new DOMException('Cancelled', 'AbortError')); };
    signal.addEventListener('abort', cancel, {once:true});
    flights.add(flight);
    maximum = Math.max(maximum, flights.size);
  }));
  h.state.usageTab = null;
  const initial = h.ui.usage();
  await tick();
  assert.equal(flights.size, 3, 'restored Show all shares the initial analytics budget');
  assert.ok(h.calls.slice(-3).some(call => call.path.includes('/summary?')));
  assert.ok(h.calls.slice(-3).some(call => call.path.includes('/events?')));
  assert.ok(h.calls.slice(-3).some(call => call.path.includes('/sections/timeseries?')));
  const input = h.document.querySelector('[name=route]');
  input.value = '/v1/'; input.focus();
  const summaryRetry = h.ui.loadUsageSummary();
  const eventsRetry = h.ui.loadUsageSection('events', true);
  await tick();
  assert.equal(maximum, 3, 'summary retries, events, charts, sections and suggestions use one budget');
  const oldCallCount = h.calls.length;
  const reapplied = h.ui.loadUsage();
  await tick();
  assert.ok(aborted > 0, 'reapply aborts previous in-flight reads');
  for (let attempt = 0; flights.size && attempt < 100; attempt += 1) {
    [...flights][0].finish();
    await tick();
  }
  await Promise.all([initial, summaryRetry, eventsRetry, reapplied]);
  assert.equal(flights.size, 0);
  assert.equal(maximum, 3);
  assert.equal(h.calls.filter(call => call.path.includes('/filter-values?')).length, 0, 'the superseded queued suggestion never starts');
  assert.ok(h.calls.slice(oldCallCount).every(call => new URL(call.path, 'http://test').searchParams.get('route') === '/v1/'), 'superseded queued breakdowns never start with the old filters');
  assert.doesNotMatch(h.document.querySelector('#usage-results').textContent, /unavailable/);
  h.dom.window.close();
}
console.log('ok - restored Show all, critical retries, charts and suggestions share one budget; reapply cancels queued work');

{
  const slow = deferred();
  let summaryCalls = 0;
  const h = harness(path => path.includes('/summary?') ? (++summaryCalls === 1 ? slow.promise : { request_count: 42 }) : { rows: [], has_more: false });
  const first = h.ui.usage();
  await tick();
  const original = h.calls.find(call => call.path.includes('/summary?'));
  await h.ui.loadUsage();
  assert.equal(original.options.signal.aborted, true);
  slow.resolve({ request_count: 999 });
  await first;
  assert.equal(h.document.querySelector('[data-usage-summary] .monitor-kpi strong').textContent, '42');
  assert.doesNotMatch(h.document.querySelector('[data-usage-summary]').textContent, /999/);
  const field = h.document.querySelector('[name=route]');
  field.value = '/exact-current';
  field.focus();
  await tick();
  const suggestion = h.calls.find(call => call.path.includes('/filter-values?'));
  assert.ok(suggestion);
  const params = new URL(suggestion.path, 'http://test').searchParams;
  assert.equal(params.get('q'), '/exact-current');
  assert.equal(params.get('field'), 'route');
  assert.equal(params.has('route'), false, 'prefix suggestions must not also require an exact match');
  field.value = '/v'; field.dispatchEvent(new h.dom.window.Event('input'));
  field.value = '/v1'; field.dispatchEvent(new h.dom.window.Event('input'));
  await new Promise(resolve => setTimeout(resolve, 280));
  assert.equal(h.calls.filter(call => call.path.includes('/filter-values?')).length, 2);
  h.state.projects = [{id:'p',name:'Project'}];
  h.state.keys = [{id:'k',name:'Worker',key_prefix:'safe-prefix'}];
  const labels = h.ui.usageIdentifierLabels();
  assert.equal(h.ui.usageIdentifierLabels(), labels);
  assert.equal(h.ui.usageProjectName('p'), 'Project');
  assert.equal(h.ui.usageKeyName('k'), 'Worker (safe-prefix)');
  h.dom.window.close();
}
console.log('ok - Usage cancels superseded reads, rejects stale completions, debounces scoped prefix suggestions and reuses identifier maps');

{
  const notices = [];
  const chunks = [];
  let request;
  const viewController = new AbortController();
  const window = { showSaveFilePicker: async () => ({createWritable: async () => new WritableStream({write(chunk) { chunks.push(chunk); }})}) };
  const fetch = async (path, options) => {
    request = {path, options};
    return { ok:true, body:new ReadableStream({start(controller) { controller.enqueue(new TextEncoder().encode('first')); controller.enqueue(new TextEncoder().encode('second')); controller.close(); }}), blob() { throw new Error('must not buffer all rows'); } };
  };
  const download = new Function('window','viewController','fetch','usageExportHeaders','setNotice','token', `${extract('usageExportPath')}\n${extract('downloadAllUsageExport')}; return downloadAllUsageExport;`)(window,viewController,fetch,()=>({authorization:'test-fixture'}),message=>notices.push(message),()=>true);
  await download('json',new URLSearchParams({from:'2026-10-22T00:00:00Z',to:'2026-10-23T00:00:00Z'}));
  assert.equal(new URL(request.path,'http://test').searchParams.get('stream'),'true');
  assert.equal(chunks.length,2);
  assert.equal(notices.at(-1),'Usage export saved.');
  assert.doesNotMatch(extract('downloadAllUsageExport'), /response\.(blob|text|json|arrayBuffer)\(|const parts =/);
}
console.log('ok - All rows export pipes response chunks to disk without accumulating a Blob or response body');
