import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import http from 'node:http';
const source = readFileSync(new URL('../crates/gateway-api/admin-ui/src/monitoring.ts', import.meta.url), 'utf8');
// This helper intentionally uses JavaScript syntax so CI can test it without npm dependencies.
export const { keyLifecycle, reliability, fetchComplete } = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
const now = Date.parse('2026-09-05T00:00:00Z');
assert.equal(keyLifecycle({expires_at:'2026-09-05T00:00:00Z'},now),'expired');
assert.equal(keyLifecycle({expires_at:'2026-09-05T00:00:00.001Z'},now),'active');
assert.equal(keyLifecycle({expires_at:null},now),'non-expiring');
assert.equal(keyLifecycle({revoked_at:'2026-09-01',disabled:true},now),'revoked');
assert.equal(keyLifecycle({disabled:true},now),'disabled');
assert.equal(reliability({request_count:0}).tone,'neutral');
assert.equal(reliability({request_count:19,error_count:19}).tone,'neutral');
assert.equal(reliability({request_count:20,error_count:1}).tone,'warn');
assert.equal(reliability({request_count:20,error_count:2}).tone,'bad');
assert.equal(reliability({request_count:21000,error_count:20,timeout_count:1}).tone,'good');
assert.equal(reliability({request_count:100,fallback_count:6}).signal,.06);

const server = http.createServer((req,res) => {
  if(req.url === '/stalled') {res.writeHead(200,{'Content-Type':'application/json'});res.write('{');return;}
  if(req.url === '/readiness') {res.writeHead(503,{'Content-Type':'application/json'});res.end('{"status":"not_ready"}');return;}
  res.writeHead(204);res.end();
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const base = `http://127.0.0.1:${server.address().port}`;
try {
  await assert.rejects(fetchComplete(`${base}/stalled`,{}, {timeoutMs:30}),/request_timeout/);
  const caller = new AbortController();
  const pending = fetchComplete(`${base}/stalled`,{signal:caller.signal},{timeoutMs:500});
  caller.abort();
  await assert.rejects(pending,{name:'AbortError'});
  const navigation = new AbortController();
  const pendingNavigation = fetchComplete(`${base}/stalled`,{}, {signal:navigation.signal,timeoutMs:500});
  navigation.abort();
  await assert.rejects(pendingNavigation,{name:'AbortError'});
  const ready = await fetchComplete(`${base}/readiness`);
  assert.equal(ready.status,503);assert.deepEqual(await ready.json(),{status:'not_ready'});
  assert.equal((await fetchComplete(`${base}/empty`)).status,204);
} finally {server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}
console.log('ok - lifecycle boundaries, sampled reliability, readiness body, stalled body timeout and composed cancellation');
