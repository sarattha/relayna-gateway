import { createHash } from "node:crypto";

export function parseMix(value, fallbackBytes) {
  const mix = value ? JSON.parse(value) : [{ bytes: fallbackBytes, weight: 1, shape: "string" }];
  if (!Array.isArray(mix) || !mix.length || mix.length > 16) throw new Error("invalid body mix");
  const seen = new Set();
  let totalBytes = 0;
  for (const item of mix) {
    item.shape ??= "string";
    if (!Number.isSafeInteger(item.bytes) || item.bytes < 64 || item.bytes > 100 * 1024 ** 2 ||
        !Number.isSafeInteger(item.weight) || item.weight < 1 || item.weight > 10000 ||
        !["string", "objects"].includes(item.shape)) throw new Error("invalid body mix entry");
    item.id = `${item.bytes}-${item.shape}`;
    if (seen.has(item.id)) throw new Error("duplicate body class");
    seen.add(item.id);
    totalBytes += item.bytes;
  }
  if (totalBytes > 256 * 1024 ** 2) throw new Error("driver payload pool exceeds 256 MiB");
  return mix;
}

export function payload(item) {
  let data;
  if (item.shape === "objects") {
    // Fixed exact length, with many small objects rather than one large string.
    const count = Math.floor((item.bytes - 32) / 8);
    const start = `{"items":[${'{"v":0},'.repeat(count)}null],"pad":"`;
    const end = '"}';
    data = Buffer.from(start + "x".repeat(item.bytes - Buffer.byteLength(start) - end.length) + end);
  } else {
    data = Buffer.alloc(item.bytes, "x");
    data.write('{"payload":"');
    data.write('"}', item.bytes - 2);
  }
  return { ...item, data, sha256: createHash("sha256").update(data).digest("hex") };
}

export function schedule(mix, count, seed = 146, order = "shuffled") {
  if (!Number.isSafeInteger(count) || count < 0 || !Number.isInteger(seed) || seed < 1 || seed > 0xffffffff ||
      !["shuffled", "large-first", "small-first"].includes(order)) throw new Error("invalid schedule");
  const total = mix.reduce((sum, item) => sum + item.weight, 0);
  const counts = mix.map((item, index) => ({ index, count: Math.floor(count * item.weight / total), remainder: count * item.weight % total }));
  let missing = count - counts.reduce((sum, item) => sum + item.count, 0);
  for (const item of [...counts].sort((a, b) => b.remainder - a.remainder || a.index - b.index)) {
    if (missing-- > 0) item.count++;
  }
  const sequence = counts.flatMap(item => Array(item.count).fill(item.index));
  let state = seed >>> 0;
  for (let i = sequence.length - 1; i > 0; i--) {
    state ^= state << 13; state ^= state >>> 17; state ^= state << 5;
    const j = (state >>> 0) % (i + 1);
    [sequence[i], sequence[j]] = [sequence[j], sequence[i]];
  }
  if (order !== "shuffled") sequence.sort((a, b) => (mix[a].bytes - mix[b].bytes) * (order === "small-first" ? 1 : -1));
  return sequence;
}

export function summarizeClasses(results, mix, seconds, distribution) {
  return mix.map(item => {
    const rows = results.filter(row => row.body_class === item.id);
    const successes = rows.filter(row => row.status === 200 && row.verified);
    const rejections = rows.filter(row => row.status === 429 || row.status === 503);
    const statusCounts = {};
    for (const row of rows) statusCounts[row.status] = (statusCounts[row.status] ?? 0) + 1;
    return { id: item.id, body_bytes: item.bytes, shape: item.shape, offered: rows.length,
      verified: successes.length, successful_bytes: successes.length * item.bytes,
      success_rps_including_drain: successes.length / seconds,
      successful_mib_per_second: successes.length * item.bytes / 1024 ** 2 / seconds,
      status_counts: statusCounts, success_latency_ms: distribution(successes.map(row => row.ms)),
      rejection_latency_ms: distribution(rejections.map(row => row.ms)) };
  });
}
