import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";
import ts from "typescript";

const read = path => readFileSync(new URL(path, import.meta.url), "utf8");
const compile = code => ts.transpileModule(code, { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 } }).outputText;
const url = code => `data:text/javascript;base64,${Buffer.from(code).toString("base64")}`;
const componentUrl = url(compile(read("../crates/gateway-api/admin-ui/src/design-system/components.ts")));
const { drawerHeading, mountDrawerNavigation, prepareDrawerActions } = await import(url(compile(read("../crates/gateway-api/admin-ui/src/design-system/workflows.ts")).replace('"./components"', JSON.stringify(componentUrl))));
const { policyLayerCards } = await import(url(compile(read("../crates/gateway-api/admin-ui/src/policy-visuals.ts")).replace('"./design-system/components"', JSON.stringify(componentUrl))));
const { fieldSummary, fieldGuidance } = await import(url(compile(read("../crates/gateway-api/admin-ui/src/design-system/guidance-content.ts"))));

{
  const dom = new JSDOM(`<div role="dialog">${drawerHeading('<img src=x onerror="bad()">', '<button type="button">Close</button>', 'drawer-title')}<div class="drawer-body"><form><details class="form-section" open><summary><strong>Identity</strong></summary><label>Name<input name="name" value="Unsaved draft"></label></details><details class="form-section"><summary><strong>Access and limits</strong></summary><label>RPM<input name="rpm" value="0"></label></details><button type="submit">Save</button></form></div></div>`, { pretendToBeVisual: true });
  const doc = dom.window.document, dialog = doc.querySelector('[role="dialog"]');
  const form = doc.querySelector('form'), originalInput = form.elements.name, originalForm = form;
  let scrolls = 0, submits = 0;
  doc.querySelector('.drawer-body').scrollTo = () => scrolls++;
  form.addEventListener('submit', event => { event.preventDefault(); submits++; });
  const before = [...new dom.window.FormData(form)];
  const dispose = mountDrawerNavigation(dialog);
  prepareDrawerActions(doc.querySelector('.drawer-body'));
  prepareDrawerActions(doc.querySelector('.drawer-body'));
  assert.equal(form.querySelectorAll('.form-actions').length, 1, 'a form gets one action row without cloning buttons');
  const buttons = [...doc.querySelectorAll('.drawer-nav button')];
  assert.equal(buttons.length, 2);
  buttons[1].click();
  assert.equal(doc.querySelectorAll('details')[1].open, true);
  assert.equal(doc.activeElement, doc.querySelectorAll('summary')[1]);
  assert.equal(buttons[1].getAttribute('aria-current'), 'location');
  assert.equal(buttons[0].hasAttribute('aria-current'), false);
  assert.equal(scrolls, 1);
  assert.equal(submits, 0, 'section navigation never submits a form');
  assert.equal(doc.querySelector('form'), originalForm);
  assert.equal(form.elements.name, originalInput, 'navigation never clones bound controls');
  assert.deepEqual([...new dom.window.FormData(form)], before, 'zero limits and unsaved fields survive navigation');
  form.dispatchEvent(new dom.window.Event('submit', { cancelable: true }));
  assert.equal(submits, 1, 'the original submission handler remains attached');
  dispose(); dispose();
  assert.equal(doc.querySelector('.drawer-nav'), null);
  assert.equal(form.elements.name.value, 'Unsaved draft');
  assert.equal(doc.querySelector('img,[onerror]'), null);
  dom.window.close();
}
{
  const unsafe = '<img src=x onerror="bad()">';
  const rows = [{ id: unsafe, kind: 'global', scope_id: null, updated_at: 'timestamp', policy: { rpm_limit: 0, daily_budget_usd: 0, allow_streaming: false, allowed_routes: [unsafe], policy_version: 3 }, guardrail_policy: {} }, { id: 'legacy', kind: '__proto__', policy: {}, guardrail_policy: {} }];
  const dom = new JSDOM(policyLayerCards(rows, () => '<span>No explicit guardrails</span>', value => value || 'Not recorded'));
  const doc = dom.window.document;
  assert.equal(doc.querySelectorAll('.policy-layer-card').length, 2);
  assert.equal(doc.querySelector('img,[onerror]'), null);
  assert.equal(doc.querySelector('[data-policy-layer-action]').dataset.layerId, unsafe);
  assert.match(doc.body.textContent, /Gateway-wide/);
  assert.match(doc.body.textContent, /Denied at this layer/);
  assert.match(doc.body.textContent, /Updated timestamp/);
  assert.match(doc.body.textContent, /__proto__/);
  const values = [...doc.querySelectorAll('dd')].map(e => e.textContent);
  assert.equal(values.filter(v => v === '0').length, 2, 'zero limits remain explicit constraints');
  assert.match(doc.body.textContent, /No explicit constraints recorded/);
  dom.window.close();
}
for (const name of ['rpm_limit', 'daily_budget_usd', 'max_tokens_per_day', 'max_request_body_bytes']) {
  assert.match(fieldSummary(name, 'policy'), /0/);
  assert.match(fieldSummary(name, 'policy'), /blank inherits/);
  assert.ok(fieldGuidance(name, 'policy').length > fieldSummary(name, 'policy').length, 'full definitions remain available');
}
assert.match(fieldSummary('max_request_body_bytes', 'service'), /greater than 0/);
assert.match(fieldSummary('credential', 'service'), /Write-only.*blank keeps/);
assert.equal(fieldSummary('unknown', 'policy'), undefined);
console.log('ok - drawer navigation preserves bound forms, draft values and zero limits; policy cards escape metadata and keep complete constraints');
