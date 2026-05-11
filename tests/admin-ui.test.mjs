import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const uiDir = join(root, "crates/gateway-api/src/static/admin-ui");
const html = readFileSync(join(uiDir, "index.html"), "utf8");
const js = readFileSync(join(uiDir, "app.js"), "utf8");
const css = readFileSync(join(uiDir, "app.css"), "utf8");

function test(name, fn) {
  try {
    fn();
    console.log(`ok - ${name}`);
  } catch (error) {
    console.error(`not ok - ${name}`);
    throw error;
  }
}

test("admin portal shell exposes all release-critical views", () => {
  for (const view of ["overview", "keys", "routes", "services", "usage", "health"]) {
    assert.match(html, new RegExp(`data-view="${view}"`));
  }
  assert.match(html, /id="operator-token"/);
  assert.match(html, /id="rotate-token"/);
});

test("admin portal calls the expected gateway admin APIs", () => {
  for (const endpoint of [
    "/admin/usage/summary",
    "/admin/provider-health",
    "/admin/openai-routes",
    "/admin/keys",
    "/admin/services",
    "/admin/operator-token/rotate",
    "/readyz",
  ]) {
    assert.match(js, new RegExp(endpoint.replaceAll("/", "\\/")));
  }
});

test("routes view includes service route registrations", () => {
  assert.match(js, /Registered service routes/);
  assert.match(js, /function serviceRouteTable\(rows\)/);
  assert.match(js, /route_pattern/);
  assert.match(js, /allowed_methods/);
});

test("service methods use explicit checkbox controls", () => {
  assert.match(js, /function methodSelect\(selected = \[\]\)/);
  assert.match(js, /class="checkbox-group"/);
  assert.match(js, /name="allowed_methods" type="checkbox"/);
  assert.match(js, /form\.getAll\("allowed_methods"\)/);
  assert.match(css, /\.checkbox-group/);
});

test("service editor closes after a successful save", () => {
  assert.match(
    js,
    /async function patchService\(event\) \{[\s\S]*await api\(`\/admin\/services\/\$\{serviceName\}`,[\s\S]*state\.editingServiceName = null;[\s\S]*await services\(\);[\s\S]*\}/,
  );
});

test("admin portal escapes rendered user-controlled values", () => {
  assert.match(js, /function esc\(value\)/);
  for (const replacement of ["&amp;", "&lt;", "&gt;", "&quot;", "&#039;"]) {
    assert.match(js, new RegExp(replacement.replace("&", "&")));
  }
});

test("admin portal remains usable on narrow screens", () => {
  assert.match(css, /@media \(max-width: 760px\)/);
  assert.match(css, /grid-template-columns: 1fr/);
  assert.match(css, /overflow-x: auto/);
});
