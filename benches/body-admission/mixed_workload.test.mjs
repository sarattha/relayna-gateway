import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { parseMix, payload, schedule, summarizeClasses } from "./mixed_workload.mjs";

test("mixed payloads are exact, parseable, and independently verifiable", () => {
  for (const bytes of [64, 8192, 262144, 1048576]) {
    for (const shape of ["string", "objects"]) {
      const item = payload(parseMix(JSON.stringify([{ bytes, weight: 1, shape }]))[0]);
      assert.equal(item.data.length, bytes);
      assert.equal(typeof JSON.parse(item.data), "object");
      assert.equal(item.sha256, createHash("sha256").update(item.data).digest("hex"));
    }
  }
});

test("weighted sequence preserves counts and can reproduce adversarial arrival orders", () => {
  const mix = parseMix(JSON.stringify([
    { bytes: 8192, weight: 70 }, { bytes: 262144, weight: 20 },
    { bytes: 1048576, weight: 9 }, { bytes: 10485760, weight: 1 },
  ]));
  const sequence = schedule(mix, 1000, 146);
  assert.deepEqual(mix.map((_, index) => sequence.filter(value => value === index).length), [700, 200, 90, 10]);
  assert.deepEqual(sequence, schedule(mix, 1000, 146));
  assert.notDeepEqual(sequence, schedule(mix, 1000, 147));
  assert.equal(schedule(mix, 1000, 146, "large-first")[0], 3);
  assert.equal(schedule(mix, 1000, 146, "small-first")[0], 0);
  for (const count of [1, 2, 3, 7, 103]) assert.equal(schedule(mix, count).length, count);
});

test("per-size goodput excludes corrupt responses, denials, and transport failures", () => {
  const mix = parseMix(JSON.stringify([{ bytes: 1048576, weight: 1 }]));
  const results = [
    { status: 200, verified: true, ms: 10 }, { status: 200, verified: false, ms: 20 },
    { status: 503, ms: 1 }, { status: 429, ms: 2 }, { status: 0 },
  ].map(row => ({ ...row, body_class: mix[0].id }));
  const [summary] = summarizeClasses(results, mix, 2, values => values);
  assert.equal(summary.offered, 5);
  assert.equal(summary.verified, 1);
  assert.equal(summary.success_rps_including_drain, .5);
  assert.equal(summary.successful_mib_per_second, .5);
  assert.deepEqual(summary.success_latency_ms, [10]);
  assert.deepEqual(summary.rejection_latency_ms, [1, 2]);
});

test("invalid mixtures and excessive driver allocations fail before a run", () => {
  for (const mix of [[], [{ bytes: 0, weight: 1 }], [{ bytes: 8192, weight: 0 }],
    [{ bytes: 8192, weight: 1, shape: "unknown" }],
    [{ bytes: 8192, weight: 1 }, { bytes: 8192, weight: 2 }],
    [98, 99, 100].map(n => ({ bytes: n * 1024 ** 2, weight: 1 }))]) {
    assert.throws(() => parseMix(JSON.stringify(mix)));
  }
});
