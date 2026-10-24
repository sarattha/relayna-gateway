import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";
import ts from "typescript";

const read = path => readFileSync(new URL(path, import.meta.url), "utf8");
const moduleUrl = source => `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
const { insightStrip, distributionBars, flowSummary } = await import(moduleUrl(read("../crates/gateway-api/admin-ui/src/design-system/insights.ts")));
const compile = source => ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 } }).outputText;
const contentUrl = moduleUrl(compile(read("../crates/gateway-api/admin-ui/src/design-system/guidance-content.ts")));
const guidanceSource = compile(read("../crates/gateway-api/admin-ui/src/design-system/guidance.ts")).replace(/(["'])\.\/guidance-content\1/, JSON.stringify(contentUrl));
const { installComponentGuidance } = await import(moduleUrl(guidanceSource));

{
  const unsafe = '<img src=x onerror="bad()">';
  const markup = insightStrip([{ label: unsafe, value: 0, icon: 'key" onclick="bad()', detail: unsafe }, { label: "Missing", value: null, icon: "clock" }], { label: "Selected project" })
    + distributionBars("Returned sample", [{ label: "Measured zero", value: 0 }, { label: "Unknown", value: null }, { label: unsafe, value: 8 }], { caption: unsafe })
    + flowSummary("Saved configuration", [{ label: unsafe, icon: "shield-check", value: 0 }], "Configured state is not a health probe.");
  const dom = new JSDOM(markup);
  const document = dom.window.document;
  assert.equal(document.querySelectorAll("img, [onerror], [onclick]").length, 0);
  assert.deepEqual([...document.querySelectorAll(".insight-metric strong")].map(node => node.textContent), ["0", "—"]);
  assert.equal(document.querySelector("progress").value, 0, "a measured zero remains distinct from unavailable data");
  assert.equal(document.querySelectorAll("progress").length, 2, "an unavailable category cannot become a zero bar");
  assert.equal(document.querySelector(".insight-unknown").getAttribute("aria-label"), "Unknown measurement");
  assert.equal(document.querySelector("[data-help-tooltip]").dataset.helpTooltip, unsafe, "authored help retains escaped source text");
  assert.match(document.body.textContent, /Selected project/);
  assert.equal(document.querySelectorAll(".insight-icon .ti-chart-dots").length, 1, "invalid icon names cannot become attributes");
  dom.window.close();
}

{
  const dom = new JSDOM(`<form id="key-form"><label>RPM limit<input name="rpm_limit" data-guidance-name="rpm_limit" type="number"></label></form><table><thead><tr><th>Latency</th></tr></thead></table>${insightStrip([{ label: "Saved state", value: 3, icon: "key", detail: "Authored insight definition." }])}`, { pretendToBeVisual: true });
  const document = dom.window.document;
  const priorElement = globalThis.Element, priorObserver = globalThis.MutationObserver;
  globalThis.Element = dom.window.Element;
  globalThis.MutationObserver = dom.window.MutationObserver;
  // JSDOM lacks native popovers; polyfill only that browser primitive.
  const matches = dom.window.Element.prototype.matches;
  dom.window.HTMLElement.prototype.matches = function(selector) { return selector === ":popover-open" ? this.hasAttribute("data-test-popover-open") : matches.call(this, selector); };
  dom.window.HTMLElement.prototype.showPopover = function() { this.setAttribute("data-test-popover-open", "true"); };
  dom.window.HTMLElement.prototype.hidePopover = function() { this.removeAttribute("data-test-popover-open"); };
  let dispose;
  try {
    dispose = installComponentGuidance(document);
    await new Promise(resolve => setImmediate(resolve));
    for (const selector of ["#key-form .help-trigger", "th .help-trigger"]) {
      const trigger = document.querySelector(selector);
      assert.ok(trigger, "generated field and table help exists");
      trigger.click();
      assert.equal(document.querySelectorAll('[role="tooltip"]').length, 1, "first tap opens exactly one tooltip after dynamic enhancement");
      assert.equal(document.querySelector('[role="tooltip"]').textContent, trigger.dataset.helpTooltip);
      trigger.click();
      assert.equal(document.querySelectorAll('[role="tooltip"]').length, 0, "second tap closes generated help");
    }
    const authored = document.querySelector('.insight-help');
    authored.focus();
    assert.equal(document.querySelector('[role="tooltip"]').textContent, "Authored insight definition.");
    let dialogEscapes = 0;
    dom.window.addEventListener("keydown", event => { if (event.key === "Escape") dialogEscapes++; });
    dom.window.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    assert.equal(document.querySelector('[role="tooltip"]'), null);
    assert.equal(dialogEscapes, 0, "contextual help consumes Escape before its containing dialog");
    dom.window.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    assert.equal(dialogEscapes, 1);
    const host = document.createElement("section");
    host.innerHTML = distributionBars("Dynamic sample", [{ label: "Requests", value: 2 }], { caption: "Only this loaded sample." });
    document.body.append(host);
    await new Promise(resolve => setImmediate(resolve));
    host.querySelector(".help-trigger").click();
    assert.equal(document.querySelector('[role="tooltip"]').textContent, "Only this loaded sample.", "authored definitions bind after navigation without replacement");
  } finally {
    dispose?.();
    globalThis.Element = priorElement;
    globalThis.MutationObserver = priorObserver;
    dom.window.close();
  }
}
console.log("ok - shared visual summaries preserve zero/unknown data, escaping and accessible help without duplicate handlers");
