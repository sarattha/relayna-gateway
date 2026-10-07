import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";
import ts from "typescript";

const source = readFileSync(new URL("../crates/gateway-api/admin-ui/src/main.ts", import.meta.url), "utf8");
const ast = ts.createSourceFile("main.ts", source, ts.ScriptTarget.Latest, true);
const names = ["services", "serviceEditForm", "pricingRulesEditor", "pricingRuleRow", "bindPricingRuleEditors", "syncPricingRuleEditor", "pricingRuleFromRow", "openApiEndpointPricingEditor", "bindEndpointPricingEditors", "syncEndpointPricingEditor", "serviceBody", "pricingRulesFromForm", "endpointPricingRulesFromForm", "submitService", "patchService", "policyFields", "usage", "formSection", "option", "attr", "esc", "listValue", "csv", "blankToUndefined", "nullableString", "nullableNumber", "money"];
const functions = names.map(name => {
  const node = ast.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === name);
  assert.ok(node, `missing function ${name}`);
  return node.getText(ast);
}).join("\n");
const dom = new JSDOM('<main id="content"></main>');
const { document, FormData, Event } = dom.window;
const contentHost = document.querySelector("#content");
const service = {
  name: "tiny-price", route_pattern: "/services/tiny-price/*", enabled: true,
  allowed_methods: ["POST"], timeout_ms: 60000, max_body_bytes: 1024,
  cost_mode: "fixed", estimated_cost_usd: 0.000123456,
  pricing_rules: [{ name: "tiny-rule", json_pointer: "/model", equals: "tiny", cost_mode: "fixed", estimated_cost_usd: 0.00000025 }],
  openapi_endpoints: [{ method: "POST", path_template: "/run" }],
  endpoint_pricing_rules: [{ method: "POST", path_template: "/run", cost_mode: "fixed", estimated_cost_usd: 0.0000025 }],
};
const state = { services: [], projects: [], editingServiceName: service.name, usageFilters: {} };
const writes = [];
const api = async (path, options) => {
  if (options) { writes.push({ path, ...options, body: JSON.parse(options.body) }); return {}; }
  return path.endsWith("/services") ? [service] : [];
};
// Keep the real renderers, event bindings and serializers. Stub unrelated
// identity/navigation infrastructure so native form validity is exercised.
const ui = new Function("document", "FormData", "Event", "state", "api", `
  let renderGeneration = 0;
  const content = document.querySelector("#content");
  const noop = () => {};
  const handleAsync = fn => fn;
  const serviceTypePresets = () => [{id: "custom", label: "Custom"}];
  const methodSelect = () => '<input name="allowed_methods" type="checkbox" value="POST" checked>';
  const endpointAccessFields = () => '';
  const endpointIdentityFields = () => '';
  const endpointAccessFromForm = () => ({});
  const serviceProtocolSummary = () => '';
  const serviceTable = () => '';
  const tableWrap = html => html;
  const emptyState = text => text;
  const serviceRouteOptions = () => '';
  const providerPolicySelect = () => '';
  const projectOptions = () => '';
  const keyOptions = () => '';
  const serviceOptions = () => '';
  const bindServiceTypePresets = noop, organizeView = noop, prepareProfileKeys = noop;
  const serviceAction = noop, openFoundryEditor = noop, previewServiceOpenApi = noop;
  const setNotice = noop, applyUsageFilters = noop, loadTaskUsage = noop;
  const usageExportAction = noop, syncUsageExportControls = noop, loadUsage = noop;
  ${functions}
  return {services, serviceEditForm, bindPricingRuleEditors, bindEndpointPricingEditors, serviceBody, submitService, patchService, policyFields, usage, money};
`)(document, FormData, Event, state, api);

function checkAmount(input) {
  assert.ok(input);
  for (const amount of ["0.0002", "0.000123456", "0.00000025", "0", "0.001", "0.01", "1.25", ""]) {
    input.value = amount;
    assert.equal(input.checkValidity(), true, `${input.outerHTML}: ${amount} should be valid`);
    assert.equal(input.validity.stepMismatch, false);
  }
  input.value = "-0.0002";
  assert.equal(input.validity.rangeUnderflow, true, "negative cost must remain invalid");
  input.value = "0.000123456";
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

await ui.services();
const create = document.querySelector("#service-form");
create.elements.namedItem("name").value = "tiny-price";
create.elements.namedItem("service_type").value = "custom";
create.elements.namedItem("cost_mode").value = "fixed";
checkAmount(create.elements.namedItem("estimated_cost_usd"));
create.querySelector('[data-pricing-rule-action="add"]').click();
const rule = create.querySelector("[data-pricing-rule-row]");
rule.querySelector('[data-pricing-rule-field="json_pointer"]').value = "/model";
rule.querySelector('[data-pricing-rule-field="equals"]').value = "tiny";
checkAmount(rule.querySelector('[data-pricing-rule-field="estimated_cost_usd"]'));
assert.equal(create.checkValidity(), true, "small prices must not block native submission");
await ui.submitService({ target: create, submitter: { value: "create" }, preventDefault() {} });
assert.equal(writes.at(-1).method, "POST");
assert.equal(writes.at(-1).body.estimated_cost_usd, 0.000123456);
assert.equal(writes.at(-1).body.pricing_rules[0].estimated_cost_usd, 0.000123456);

const edit = document.querySelector("#service-edit-form");
assert.equal(edit.elements.namedItem("estimated_cost_usd").value, "0.000123456", "saved default reloads exactly");
assert.equal(edit.querySelector('[data-pricing-rule-field="estimated_cost_usd"]').value, "2.5e-7", "saved rule reloads exactly");
assert.equal(edit.querySelector('[data-endpoint-field="estimated_cost_usd"]').value, "0.0000025", "saved endpoint reloads exactly");
for (const input of edit.querySelectorAll('input[type="number"][step]')) {
  if (input.name === "timeout_ms") continue;
  checkAmount(input);
}
assert.equal(edit.checkValidity(), true);
await ui.patchService({ target: edit, preventDefault() {} });
assert.equal(writes.at(-1).method, "PATCH");
for (const amount of [writes.at(-1).body.estimated_cost_usd, writes.at(-1).body.pricing_rules[0].estimated_cost_usd, writes.at(-1).body.endpoint_pricing_rules[0].estimated_cost_usd]) assert.equal(amount, 0.000123456);

document.body.innerHTML = ui.serviceEditForm({ ...service, foundry: { mode: "endpoint_passthrough" } });
ui.bindPricingRuleEditors();
const foundry = document.querySelector("form");
for (const input of foundry.querySelectorAll('[name="estimated_cost_usd"], [data-pricing-rule-field="estimated_cost_usd"]')) checkAmount(input);
assert.equal(foundry.checkValidity(), true, "Foundry uses the same valid pricing editor");
const estimate = foundry.elements.namedItem("estimated_cost_usd");
for (const [text, expected] of [["", null], ["0", 0], ["0.00000025", 0.00000025]]) {
  estimate.value = text;
  assert.equal(ui.serviceBody(new FormData(foundry), true).estimated_cost_usd, expected);
}

document.body.innerHTML = `<form>${ui.policyFields()}</form>`;
for (const field of ["daily_budget_usd", "monthly_budget_usd", "max_cost_per_request"]) checkAmount(document.querySelector(`[name="${field}"]`));
document.body.replaceChildren(contentHost);
await ui.usage();
checkAmount(contentHost.querySelector('[name="min_cost_usd"]'));

for (const [amount, expected] of [[0.0002, "$0.0002"], [0.000123456, "$0.000123456"], [0.00000025, "$0.00000025"], [0, "$0.00"], [1.25, "$1.25"], [null, "n/a"]]) assert.equal(ui.money(amount), expected);
console.log("ok - tiny service, request-rule, endpoint and Foundry prices pass native validity and POST/PATCH without rounding; policy and Usage inputs agree");
