import http from 'node:http';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {once} from 'node:events';

let service;
let mode = 'success';
let expectedSize;
const mock = spawn(process.execPath, ['/bench/mock.mjs'], {env: {...process.env, SERVICE_TOKEN: 'fixture-service', READ_DELAY_MS: '100', DELAY_MS: '10'}, stdio: ['ignore', 'ignore', 'inherit']});
const control = http.createServer(async (req, res) => {
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  const data = chunks.length ? JSON.parse(Buffer.concat(chunks)) : {};
  let value = {};
  let status = 200;
  if (req.url.endsWith('/metrics')) {
    res.end('gateway_buffered_requests 0\ngateway_buffered_body_bytes 0\n'); return;
  }
  if (req.url.endsWith('/services/body-benchmark') && req.method === 'GET') {
    status = service ? 200 : 404;
    value = service ?? {};
  } else if (req.url.endsWith('/services') || req.method === 'PATCH') {
    assert.equal(data.max_body_bytes, Math.max(expectedSize, 8 * 1048576));
    service = {...service, ...data}; value = service;
  } else if (req.url.endsWith('/keys')) {
    assert.equal(data.policy.max_request_body_bytes, Math.max(expectedSize, 8 * 1048576));
    value = {raw_key: 'fixture-key', key: {id: 'fixture-id'}};
  }
  res.writeHead(status, {'content-type': 'application/json'}); res.end(JSON.stringify(value));
});
const proxy = http.createServer((req, res) => {
  const warmup = req.url.endsWith('/warmup');
  if ((mode === 'warmup' && warmup) || (mode === 'reject' && !warmup)) {
    req.resume(); res.writeHead(warmup ? 413 : 503, {'content-type': 'application/json'});
    return res.end(JSON.stringify({error: {code: warmup ? 'body_too_large' : 'gateway_overloaded'}}));
  }
  if (mode === 'deadline' && !warmup) {req.resume(); return;}
  const upstream = http.request({host: '127.0.0.1', port: 4000, method: 'POST', path: warmup ? '/warmup' : '/work', headers: {...req.headers, authorization: 'Bearer fixture-service'}}, response => {
    const reply = () => {res.writeHead(response.statusCode, response.headers); response.pipe(res);};
    if (mode === 'silent' && !warmup) setTimeout(reply, 35000); else reply();
  });
  upstream.on('error', () => res.destroy()); req.pipe(upstream);
});
async function child(args, bodyMiB, deadline = 10000) {
  const processChild = spawn(process.execPath, ['/bench/workload.mjs', ...args], {env: {...process.env, BENCH_BODY_MIB: String(bodyMiB), BENCH_CLIENT_DEADLINE_MS: String(deadline)}, stdio: ['ignore', 'pipe', 'pipe']});
  let output = '', errors = '';
  processChild.stdout.on('data', chunk => output += chunk);
  processChild.stderr.on('data', chunk => errors += chunk);
  const [code] = await once(processChild, 'exit');
  assert.equal(code, 0, errors);
  return JSON.parse(output);
}
const config = JSON.stringify({pods: [{name: 'a', ip: '127.0.0.1'}, {name: 'b', ip: '127.0.0.1'}], concurrent_per_pod: 2, load_mode: 'simultaneous'});
try {
  await Promise.all([new Promise(resolve => control.listen(8081, resolve)), new Promise(resolve => proxy.listen(8080, resolve))]);
  for (let retry = 0; retry < 50; retry++) {
    try {await fetch('http://127.0.0.1:4000/health'); break;} catch {await new Promise(resolve => setTimeout(resolve, 20));}
  }
  for (const size of [5, 100]) {
    expectedSize = size * 1048576;
    await child(['seed', 'http://127.0.0.1:8081'], size);
    const run = await child([config], size);
    assert.equal(run.completed_requests, 4); assert.equal(run.verified_requests, 4);
    assert.equal(run.body_bytes, expectedSize);
    if (size === 5) assert.equal(run.body_sha256, '37439e9de7c7e8c68d54d1bbde5bad4aa394b147b39a683fa21d9ce6d79d36f4');
    assert(run.success_latency_ms.min >= 100);
    console.log(`PASS exact ${size} MiB upload, hash, credentials, read pause and fixture resizing`);
  }
  mode = 'reject';
  const rejected = await child([config], 5);
  assert.equal(rejected.status_counts['503'], 4);
  assert(rejected.results.every(row => row.error_code === 'gateway_overloaded'));
  console.log('PASS early rejection returns complete classified observations');
  const wideConfig = JSON.stringify({pods: [{name: 'a', ip: '127.0.0.1'}, {name: 'b', ip: '127.0.0.2'}], concurrent_per_pod: 300, load_mode: 'simultaneous'});
  const wide = await child([wideConfig], 5);
  assert.equal(wide.http_max_sockets_per_host, 300);
  assert.equal(wide.completed_requests, 600);
  assert.equal(wide.status_counts['503'], 600);
  for (const pod of ['a','b']) assert.equal(wide.results.filter(row => row.pod === pod).length, 300);
  console.log('PASS 300-request cohorts per distinct host exceed the old 256-socket cap');
  mode = 'deadline';
  const expired = await child([config], 5, 200);
  assert.equal(expired.status_counts['0'], 4);
  assert(expired.results.every(row => row.error === 'benchmark wall-clock deadline'));
  console.log('PASS absolute deadlines settle every request and exit');
  mode = 'silent';
  const silent = await child([config], 5, 60000);
  assert.equal(silent.verified_requests, 4);
  assert.equal(silent.client_socket_timeout_ms, 60000);
  assert(silent.success_latency_ms.min >= 35000);
  console.log('PASS 35-second silent response completes under the 60-second resource watchdog');
  mode = 'warmup';
  const warmup = await child([config], 5);
  assert.equal(warmup.warmup_failed, true); assert.equal(warmup.warmup_results.length, 2);
  console.log('PASS unsuccessful warmup retains structured evidence');
} finally {
  mock.kill(); control.closeAllConnections(); proxy.closeAllConnections();
  control.close(); proxy.close();
}
