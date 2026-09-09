import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import http from 'node:http';
import {fetchComplete} from './admin-ui-monitoring.test.mjs';
const source = readFileSync(new URL('../crates/gateway-api/admin-ui/src/main.ts', import.meta.url), 'utf8');
function extract(name, next) { return source.slice(source.indexOf(name), source.indexOf(next, source.indexOf(name))); }
const requests = [];
const controller = new AbortController();
const fetchSource = extract('async function fetchWithTimeout(', '\nconst monitoringCache');
const fetchBounded = new Function('fetchComplete', 'viewController', `let viewGeneration=1; const requestTimeoutMs=8000; ${fetchSource}; return fetchWithTimeout;`)(async (path, options, policy) => { requests.push({path,options,policy}); return 'ok'; }, controller);
for (const path of ['/admin-ui/admin/usage/summary?from=x', '/admin-ui/admin/usage/timeseries', '/admin-ui/admin/provider-health?from=x', '/admin-ui/admin/keys', '/admin-ui/readyz']) {
  assert.equal(await fetchBounded(path), 'ok');
  assert.equal(requests.at(-1).policy.timeoutMs, /usage|provider-health/.test(path) ? 30000 : 8000);
  assert.equal(requests.at(-1).policy.signal, controller.signal);
}
await fetchBounded('/admin-ui/admin/keys', {method:'POST'});
assert.equal(requests.at(-1).policy.signal, undefined, 'navigation does not cancel writes');
const overview = extract('async function overview()', '\nfunction overviewUsageQuery');
assert.doesNotMatch(overview, /usage\/dashboard/);
for (const endpoint of ['summary','timeseries','by-project']) assert.ok(overview.includes(`/usage/${endpoint}?`));
const state = {overviewWindow:'24h', projectScope:''};
const getQuery = new Function('state', `${extract('function overviewUsageQuery()', '\nfunction overviewWindowLabel')}; return overviewUsageQuery;`)(state);
for (const [window, limit, interval] of [['24h','25','hour'],['7d','169','hour'],['30d','31','day']]) {
  state.overviewWindow = window;
  const query = new URLSearchParams(getQuery());
  assert.equal(query.get('timeseries_limit'),limit);
  assert.equal(query.get('interval'),interval);
  assert.ok(Date.parse(query.get('to')) > Date.parse(query.get('from')));
}
state.projectScope='project-a';
assert.equal(new URLSearchParams(getQuery()).get('project_id'),'project-a');
const issues=[];
let fail=false;
const panel = new Function('api','state','location', `const monitoringCache=new Map(); ${extract('async function monitoringPanel(', '\nfunction monitoringIssues')}; return monitoringPanel;`)(async()=>{if(fail)throw new Error('request_timeout');return {request_count:19};},state,{origin:'http://localhost'});
assert.deepEqual(await panel('/admin-ui/admin/usage/summary?project_id=a&from=1',{},issues),{request_count:19});
fail=true;
assert.deepEqual(await panel('/admin-ui/admin/usage/summary?project_id=a&from=2',{},issues),{request_count:19});
assert.match(issues.at(-1),/stale data/);
assert.deepEqual(await panel('/admin-ui/admin/usage/summary?project_id=b',{},issues),{});
assert.match(issues.at(-1),/unavailable/);
const render = new Function('emptyState','esc','money','time', `${extract('function liteLlmSpendContent(', '\nasync function providers')}; return liteLlmSpendContent;`)(x=>x,x=>x,x=>`$${x.toFixed(2)}`,x=>x);
assert.match(render({status:'not_mapped'}),/No enabled LiteLLM/);
for (const spend of [null,undefined,-1,Infinity,'2']) assert.match(render({status:'available',spend_usd:spend}),/unavailable/);
assert.match(render({status:'unavailable',mapping_scope:'project'}),/Shared project mapping/);
assert.match(render({status:'available',mapping_scope:'key',spend_usd:0,fetched_at:'now'}),/\$0.00/);
assert.match(render({status:'available',mapping_scope:'project',spend_usd:12.34,fetched_at:'now'}),/Shared project mapping · Retrieved now/);
console.log('ok - bounded analytics, lean Overview, complete rolling buckets, independent stale sources and accurate spend states');

// A production-like slow response must survive the former eight-second cutoff.
const server=http.createServer((req,res)=>{setTimeout(()=>{res.setHeader('content-type','application/json');res.end('{"request_count":42}');},8100);});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
try {
  const read=new Function('fetchComplete','viewController',`let viewGeneration=1; const requestTimeoutMs=8000; ${fetchSource}; return fetchWithTimeout;`)(fetchComplete,new AbortController());
  const response=await read(`http://127.0.0.1:${server.address().port}/admin-ui/admin/usage/summary`);
  assert.deepEqual(await response.json(),{request_count:42});
} finally {server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}
console.log('ok - actual analytics response beyond eight seconds succeeds');

const activity = new Function('esc','scopeLabel','overviewWindowLabel','emptyState','table','money','attr','overviewChartSummary', `${extract('function overviewProjectActivity(', '\nfunction overviewUsageQuery')}; return {projects:overviewProjectActivity,volume:overviewRequestVolume};`)(x=>x,()=>"All projects",()=>"Last 7 days",x=>x,(_,rows)=>rows.length?JSON.stringify(rows):"No rows",x=>x,x=>x,()=>"No timeseries");
const failedProjects = activity.projects([],[],['by-project: request_timeout · unavailable']);
assert.match(failedProjects,/Project activity is unavailable/);
assert.doesNotMatch(failedProjects,/top 0|No rows/);
assert.match(activity.projects([],[],[]),/top 0 recorded projects/);
assert.match(activity.projects([{name:'project-a',summary:{request_count:7,failure_count:0,estimated_cost_usd:1}}],[{id:'project-a',name:'Support'}],['by-project: request_timeout · stale data from 10:00']),/Support/);
assert.match(activity.volume([],['timeseries: request_timeout · unavailable']),/Request volume is unavailable/);
assert.doesNotMatch(activity.volume([],['timeseries: request_timeout · unavailable']),/<canvas|No timeseries/);
assert.match(activity.volume([],[]),/<canvas/);
assert.match(activity.volume([],['timeseries: request_timeout · stale data from 10:00']),/<canvas/);
console.log('ok - unavailable Overview sections never present failed reads as zero activity; valid empty and cached data remain distinct');
