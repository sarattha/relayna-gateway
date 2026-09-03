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
