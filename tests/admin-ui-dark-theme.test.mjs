import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const source = JSON.parse(readFileSync(new URL("../docs/assets/brand/tokens.json", import.meta.url)));
const aliases = readFileSync(new URL("../crates/gateway-api/admin-ui/src/design-system/tokens.css", import.meta.url), "utf8");
const mapping = Object.fromEntries([...aliases.matchAll(/(--[\w-]+): var\(--rn-([\w-]+)\);/g)].map(([, key, role]) => [key, role]));
function luminance(hex) {
  return hex.slice(1).match(/../g).map(c => parseInt(c, 16) / 255).map(c => c <= .04045 ? c / 12.92 : ((c + .055) / 1.055) ** 2.4).reduce((sum, c, i) => sum + c * [.2126, .7152, .0722][i], 0);
}
function contrast(a, b) {
  const [lo, hi] = [luminance(a), luminance(b)].sort((a, b) => a - b);
  return (hi + .05) / (lo + .05);
}
for (const mode of ["light", "dark"]) {
  const tokens = Object.fromEntries(Object.entries(mapping).map(([key, role]) => [key, source.themes[mode][role]]));
  for (const [foreground, background, minimum] of [
    ["--foreground", "--background", 7], ["--muted-foreground", "--muted", 4.5],
    ["--primary", "--background", 4.5], ["--primary-foreground", "--rg-color-control", 4.5],
    ["--rg-color-accent-strong", "--rg-color-bg", 4.5],
    ["--rg-status-good", "--rg-status-good-bg", 4.5], ["--rg-status-bad", "--rg-status-bad-bg", 4.5],
    ["--rg-status-warn", "--rg-status-warn-bg", 4.5], ["--rg-chart-text", "--background", 4.5],
    ["--rg-chart-teal", "--background", 3], ["--rg-chart-blue", "--background", 3],
    ["--rg-chart-violet", "--background", 3], ["--rg-chart-red", "--background", 3],
    ["--rg-color-border-strong", "--background", 3],
  ]) {
    assert.ok(tokens[foreground] && tokens[background], `${mode}: unresolved role`);
    assert.ok(contrast(tokens[foreground], tokens[background]) >= minimum, `${mode}: ${foreground}/${background} >= ${minimum}:1`);
  }
}
console.log("ok - Mulberry & Linen light/dark adapters preserve text, action, status, control and chart contrast");
