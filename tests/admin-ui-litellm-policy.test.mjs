import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";
const source = readFileSync(new URL("../crates/gateway-api/admin-ui/src/main.ts", import.meta.url), "utf8");
function extract(name, next) {
  const start = source.indexOf(`function ${name}(`);
  return source.slice(start, source.indexOf(`\n${next}`, start));
}
const render = extract("litellmPassthroughForm", "function providerTable");
const saveStart = source.indexOf("async function saveLiteLlmPassthroughSettings(");
const save = source.slice(saveStart, source.indexOf("\nfunction ", saveStart));
const escape = v => String(v ?? "").replaceAll("&", "&amp;").replaceAll('"', "&quot;").replaceAll("<", "&lt;");
let persisted = { enabled: true, allowed_paths: ["/*"], allowed_methods: ["GET", "POST"], ui_exposure: "disabled", admin_api_exposure: "explicitly_exposed" };
const dom = new JSDOM("<main></main>");
let reloads = 0;
const api = async (path, options) => {
  assert.equal(path, "/admin-ui/admin/providers/litellm-passthrough");
  assert.equal(options.method, "PATCH");
  persisted = JSON.parse(options.body);
};
const factory = new Function("formSection", "attr", "listValue", "option", "FormData", "api", "csv", "nullableNumber", "setNotice", "providers",
  `${render}\n${save}\nreturn {litellmPassthroughForm,saveLiteLlmPassthroughSettings};`);
const functions = factory((title, help, content) => `<fieldset><legend>${title}</legend>${content}</fieldset>`, escape,
  (list, fallback) => list?.join(",") ?? fallback, (value, selected) => `<option value="${value}" ${selected === value ? "selected" : ""}>${value}</option>`,
  dom.window.FormData, api, value => value.split(",").map(s => s.trim()).filter(Boolean), value => Number(value), () => {}, async () => { reloads++; });
function mount() { dom.window.document.querySelector("main").innerHTML = functions.litellmPassthroughForm(persisted); return dom.window.document.querySelector("form"); }
let form = mount();
assert.equal(form.elements.authentication_mode.value, "gateway");
assert.equal(form.elements.blocked_paths.value, "");
form.elements.authentication_mode.value = "litellm_bearer";
form.elements.blocked_paths.value = "/key/delete, /config, /config/*";
await functions.saveLiteLlmPassthroughSettings({ preventDefault() {}, target: form });
assert.equal(reloads, 1);
form = mount();
assert.equal(form.elements.authentication_mode.value, "litellm_bearer");
assert.deepEqual(persisted.blocked_paths, ["/key/delete", "/config", "/config/*"]);
assert.equal(form.elements.ui_exposure.value, "disabled");
assert.equal(form.elements.admin_api_exposure.value, "explicitly_exposed");
for (const name of ["authentication_mode", "blocked_paths"]) {
  const control = form.elements[name];
  assert.ok(control.closest("label").textContent.trim());
  assert.ok(dom.window.document.getElementById(control.getAttribute("aria-describedby")).textContent.trim());
}
form.elements.blocked_paths.value = "";
await functions.saveLiteLlmPassthroughSettings({ preventDefault() {}, target: form });
assert.deepEqual(persisted.blocked_paths, []);
persisted.blocked_paths = ['"><img src=x onerror=alert(1)>'];
form = mount();
assert.equal(form.querySelector("img"), null);
console.log("ok - LiteLLM policy form saves and reloads independent auth/exposure/deny settings");
