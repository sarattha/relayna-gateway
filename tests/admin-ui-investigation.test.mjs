import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";
const source = readFileSync(new URL("../crates/gateway-api/admin-ui/src/investigation.ts", import.meta.url), "utf8");
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 } }).outputText;
const { routingModeLabel, usageValue, timingValue, matchTrafficRecord, requestInvestigationView, investigationUsageSnapshot } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString("base64")}`);
assert.deepEqual(investigationUsageSnapshot({request_id:'safe',endpoint_path:'/users/private@example.test',authorization:'secret',body:'private prompt'}),{request_id:'safe'});
assert.equal(timingValue({connection_reused:true,tcp_connect_us:120},"tcp_connect_us"),"Reused connection");
assert.equal(timingValue({tls:false},"tls_handshake_us"),"Not used · HTTP");
assert.equal(timingValue({dns_status:"ip_literal"},"dns_us"),"Not needed · IP address");
assert.equal(timingValue({},"response_headers_ms"),"Not recorded");
assert.equal(timingValue({response_headers_ms:0},"response_headers_ms"),"0 ms");
assert.equal(timingValue({tcp_connect_us:850},"tcp_connect_us"),"0.85 ms");
const usage={request_id:"reused",key_id:"key-a",project_id:"project-a",diagnostics:{traffic_id:"id-a"}};
const rows=[{id:"id-b",request_id:"reused",key_id:"key-a",project_id:"project-a"},{id:"id-a",request_id:"reused",key_id:"key-a",project_id:"project-a"}];
assert.equal(matchTrafficRecord(rows,usage),rows[1]);
assert.equal(matchTrafficRecord(rows,{...usage,diagnostics:{}}),null);
assert.equal(matchTrafficRecord(rows,{...usage,key_id:"another-key"}),null);
const esc=v=>String(v).replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;').replaceAll('"','&quot;');
const helpers={esc,time:String,table:(headers,rows)=>JSON.stringify({headers,rows})};
const partial=requestInvestigationView({usage:{request_id:'<img src=x onerror=alert(1)>',status_code:200,total_tokens:0},notice:'<script>oops</script>'},helpers);
assert.ok(!partial.includes('<img'));
assert.ok(!partial.includes('<script>'));
assert.ok(partial.includes('Not recorded'));
assert.ok(partial.includes('Raw diagnostics &amp; hashes') || partial.includes('Raw diagnostics & hashes'));
const denied=requestInvestigationView({traffic:{request_id:'denied',attempts:0,completed:true,client_status:401,diagnostics:{failure_code:'invalid_virtual_key'},timeline:[],upstream_timings:[]}},helpers);
assert.ok(denied.includes('No upstream connection was attempted.'));
assert.ok(denied.includes('could not be authenticated'));
const interrupted=requestInvestigationView({traffic:{request_id:'stream',completed:true,streaming:true,client_status:200,diagnostics:{failure_code:'upstream_connection_closed',outcome:'stream_interrupted'},timeline:[],upstream_timings:[],recording_failures:['usage_events']}},helpers);
assert.ok(interrupted.includes('>Failed<'), 'a stream can fail after HTTP 200');
assert.ok(interrupted.includes('Recording gaps: usage_events'));
console.log('ok - shared investigation distinguishes missing/reused timings, exact correlation, stream failure and escaped metadata');

const pass = { routing_mode: 'litellm_passthrough' };
assert.equal(routingModeLabel(pass), 'LiteLLM passthrough');
assert.equal(routingModeLabel({routing_mode:'managed_by_gateway'}), 'Gateway managed');
assert.equal(routingModeLabel({}), 'Not recorded');
assert.equal(routingModeLabel({routing_mode:'future_mode'}), 'Not recorded');
assert.equal(routingModeLabel({routing_mode:'constructor'}), 'Not recorded');
assert.equal(usageValue(pass, null), 'Not metered by gateway');
assert.equal(usageValue(pass, 0), 'Not metered by gateway');
assert.equal(usageValue({}, null), 'Not recorded');
assert.equal(usageValue({routing_mode:'managed_by_gateway'}, 0), '0');
const noIdentity = requestInvestigationView({traffic:{request_id:'direct',diagnostics:pass,completed:true,client_status:200,attempts:1}},helpers);
assert.ok(noIdentity.includes('LiteLLM passthrough'));
assert.equal((noIdentity.match(/Not metered by gateway/g)||[]).length, 4);
const servicePricing = requestInvestigationView({usage:{request_id:'service',provider:'litellm',cost_mode:'passthrough',estimated_cost_usd:0,total_tokens:0}},helpers);
assert.ok(!servicePricing.includes('Not metered by gateway'), 'service passthrough pricing does not imply LiteLLM routing mode');
assert.ok(servicePricing.includes('$0.000000'));
const legacyMode = requestInvestigationView({usage:{request_id:'old',route:'/litellm/*',provider:'litellm',cost_source:'none'}},helpers);
assert.ok(!legacyMode.includes('Not metered by gateway'), 'legacy mode must not be guessed from route/provider/cost source');
console.log('ok - routing labels distinguish passthrough, zero values, missing data and keyless Traffic');

const { websocketMetrics, refreshWebSocketMetrics } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString("base64")}`);
const socket = {state:"open",opened_at:"2026-09-16T00:00:00Z",observed_at:"2026-09-16T00:00:02Z",last_upstream_activity_at:"2026-09-16T00:00:02Z",session_expires_at:"2026-09-16T00:01:00Z",idle_timeout_ms:10000,client_bytes:1000,upstream_bytes:2000,client_frames:2,upstream_frames:4,max_frame_bytes:4096};
const liveMetrics = websocketMetrics(socket,true,Date.parse("2026-09-16T00:00:05Z"));
assert.equal(liveMetrics.duration,"5 s");
assert.equal(liveMetrics.upload_rate,"200 B/s");
assert.equal(liveMetrics.idle,"7 s");
assert.equal(liveMetrics.expiry,"55 s");
assert.equal(websocketMetrics(socket,true,Date.parse("2026-09-16T00:02:00Z")).expiry,"Deadline reached · awaiting enforcement");
assert.equal(websocketMetrics({...socket,state:"closed",closed_at:"2026-09-16T00:00:03Z"},true).duration,"3 s");
assert.equal(websocketMetrics(socket,false).idle,"Not active");
assert.equal(websocketMetrics({state:"connecting"}).duration,"Not opened");
const identity={policy_source:"endpoint",verification:"verified",expected_audience:"api://accessa",audiences:["api://accessa"],scopes:["run"],roles:["<script>role</script>"],groups:["staff"],required_scopes:["run"],required_roles:[],allowed_groups:["staff"],expires_at:2000000000,truncated:true};
for (const input of [
  {traffic:{...rows[0],started_at:socket.opened_at,completed:false,diagnostics:{websocket:socket,entra:identity},timeline:[]}},
  {usage:{request_id:"socket-usage",status_code:101,diagnostics:{websocket:{...socket,state:"closed",closed_at:socket.observed_at},entra:identity}}},
]) {
  const html=requestInvestigationView(input,helpers);
  for(const expected of ["WebSocket session","Entra verification","api://accessa","Verified scopes","Required scopes","Average upload","Claim display was truncated"]) assert.ok(html.includes(expected),expected);
  assert.ok(!html.includes("<script>role"));
}
const field={dataset:{websocketMetric:"duration"},textContent:""};
refreshWebSocketMetrics({querySelectorAll:()=>[{dataset:{websocketLive:JSON.stringify(socket)},querySelectorAll:()=>[field]}]},Date.parse("2026-09-16T00:00:05Z"));
assert.equal(field.textContent,"5 s");
console.log("ok - WebSocket live countdowns, terminal duration, transfer rates and safe Entra snapshots in Traffic and Usage");
