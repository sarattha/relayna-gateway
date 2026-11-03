import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import { JSDOM } from "jsdom";
const materialCss = readFileSync(new URL("../docs/assets/brand/material-tokens.css", import.meta.url), "utf8");
const paletteDom = new JSDOM(`<style>${materialCss}</style>`);
const rules = paletteDom.window.document.styleSheets[0].cssRules;
assert.equal(rules.length, 2, "both documentation theme blocks must parse independently");
for (const [index, scheme, primitive] of [[0, "default", "linen"], [1, "slate", "ink"]]) {
  assert.equal(rules[index].selectorText, `[data-md-color-scheme="${scheme}"]`);
  assert.equal(rules[index].style.getPropertyValue("--rn-color-canvas"), `var(--rn-primitive-${primitive})`);
}
paletteDom.window.close();
const dom = new JSDOM('<body data-md-color-scheme="default"><pre class="mermaid">flowchart LR\n A--&gt;B</pre></body>');
const { window } = dom;
const themes = [], inputs = [];
let navigation;
window.document$ = { subscribe(fn) { navigation = fn; } };
window.mermaid = {
  initialize(config) { themes.push(config); },
  async run({ nodes }) {
    inputs.push(nodes.map(node => node.textContent));
    for (const node of nodes) { node.innerHTML = '<svg role="img"></svg>'; node.dataset.processed = "true"; }
  },
};
const colors = { canvas: "#F7F0E8", surface: "#FFFCF8", text: "#2C1F2E", "border-control": "#9C8296", "surface-subtle": "#EFE4E9", "surface-selected": "#EADAE5", "text-muted": "#6E5B6A" };
const source = readFileSync(new URL("../docs/assets/mermaid-init.js", import.meta.url), "utf8");
vm.runInNewContext(source, { window, document: window.document, MutationObserver: window.MutationObserver, getComputedStyle: () => ({ getPropertyValue: name => colors[name.replace("--rn-color-", "")] || "" }) });
navigation();
await new Promise(resolve => setImmediate(resolve));
assert.equal(themes[0].theme, "base");
assert.equal(themes[0].securityLevel, "strict");
assert.equal(themes[0].themeVariables.primaryTextColor, "#2C1F2E");
assert.match(themes[0].themeVariables.fontFamily, /Manrope/);
colors.text = "#F7F0E8";
window.document.body.dataset.mdColorScheme = "slate";
await new Promise(resolve => setImmediate(resolve));
assert.equal(themes.at(-1).themeVariables.darkMode, true);
assert.equal(themes.at(-1).themeVariables.primaryTextColor, "#F7F0E8");
assert.equal(inputs.length, 2);
assert.equal(inputs[0][0], inputs[1][0], "a theme switch rerenders the original diagram, never its SVG output");
navigation();
await new Promise(resolve => setImmediate(resolve));
assert.equal(inputs.at(-1)[0], inputs[0][0], "instant navigation preserves existing source");
dom.window.close();
console.log("ok - documentation diagrams use shared colors/type and retain original source across theme and navigation changes");
