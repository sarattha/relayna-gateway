import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const css = readFileSync(new URL("../crates/gateway-api/admin-ui/src/theme.css", import.meta.url), "utf8");
const dark = css.match(/:root\[data-theme="dark"\]\s*\{([^}]+)\}/)[1];
const tokens = Object.fromEntries([...dark.matchAll(/(--[\w-]+):\s*(#[0-9a-f]{6});/gi)].map(([, key, value]) => [key, value]));
function luminance(hex) {
  const rgb = hex.slice(1).match(/../g).map(channel => parseInt(channel, 16) / 255)
    .map(channel => channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4);
  return rgb.reduce((sum, channel, index) => sum + channel * [0.2126, 0.7152, 0.0722][index], 0);
}
function contrast(foreground, background) {
  const [low, high] = [luminance(foreground), luminance(background)].sort((a, b) => a - b);
  return (high + 0.05) / (low + 0.05);
}
for (const [foreground, background, minimum] of [
  ["--foreground", "--background", 7],
  ["--muted-foreground", "--muted", 4.5],
  ["--primary", "--background", 4.5],
  ["--rg-status-good", "--rg-status-good-bg", 4.5],
  ["--rg-status-bad", "--rg-status-bad-bg", 4.5],
  ["--rg-status-warn", "--rg-status-warn-bg", 4.5],
  ["--rg-chart-text", "--background", 4.5],
  ["--rg-chart-teal", "--background", 3],
  ["--rg-chart-blue", "--background", 3],
  ["--rg-chart-violet", "--background", 3],
  ["--rg-chart-red", "--background", 3],
]) {
  assert.ok(contrast(tokens[foreground], tokens[background]) >= minimum, `${foreground} on ${background} must meet ${minimum}:1`);
}
assert.ok(contrast("#F1F8F3", tokens["--rg-color-control"]) >= 4.5, "primary button labels must remain readable on dark green");
assert.ok(contrast(tokens["--rg-color-border-strong"], "#0E1B16") >= 3, "input boundaries must remain visible");
console.log("ok - dark green palette preserves readable text, semantic statuses, input boundaries and chart series");
