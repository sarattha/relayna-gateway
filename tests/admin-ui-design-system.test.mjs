import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));

function read(path) {
  return readFileSync(join(root, path), "utf8");
}

function test(name, fn) {
  try {
    fn();
    console.log(`ok - ${name}`);
  } catch (error) {
    console.error(`not ok - ${name}`);
    throw error;
  }
}

const packageJson = read("package.json");
const viteConfig = read("crates/gateway-api/admin-ui/vite.config.ts");
const sourceHtml = read("crates/gateway-api/admin-ui/index.html");
const designSystem = read("crates/gateway-api/admin-ui/src/design-system.ts");
const sourceCss = read("crates/gateway-api/admin-ui/src/app.css");
const generatedHtml = read("crates/gateway-api/src/static/admin-ui/index.html");
const generatedJs = read("crates/gateway-api/src/static/admin-ui/app.js");
const generatedCss = read("crates/gateway-api/src/static/admin-ui/app.css");

test("admin ui source is built through Vite into the existing static asset contract", () => {
  assert.match(packageJson, /"build:admin-ui"/);
  assert.match(packageJson, /vite build --config crates\/gateway-api\/admin-ui\/vite\.config\.ts/);
  assert.match(viteConfig, /base: "\/admin-ui\/"/);
  assert.match(viteConfig, /outDir: "\.\.\/src\/static\/admin-ui"/);
  assert.match(viteConfig, /entryFileNames: "app\.js"/);
  assert.match(viteConfig, /return "app\.css"/);
  assert.match(generatedHtml, /\/admin-ui\/app\.js/);
  assert.match(generatedHtml, /\/admin-ui\/app\.css/);
});

test("admin ui source defines governance domains for every release-critical view", () => {
  for (const view of ["overview", "providers", "services", "routes", "projects", "keys", "guardrails", "usage", "health", "settings"]) {
    assert.match(sourceHtml, new RegExp(`data-view="${view}"`));
    assert.match(designSystem, new RegExp(`${view}: \\{`));
  }
  for (const domain of ["Monitor", "Discover", "Govern"]) {
    assert.match(sourceHtml, new RegExp(`<span>${domain}<\\/span>`));
    assert.match(designSystem, new RegExp(`domain: "${domain}"`));
  }
});

test("admin ui design system exposes 2.0 tokens and operator-console components", () => {
  for (const token of ["--rg-color-bg", "--rg-color-surface", "--rg-color-accent", "--rg-status-good", "--rg-status-bad", "--rg-focus-ring"]) {
    assert.match(sourceCss, new RegExp(token));
    assert.match(generatedCss, new RegExp(token));
  }
  for (const className of ["nav-group", "metric-strip", "panel-heading", "badge good", "modal-backdrop", "table-wrap", "form-grid"]) {
    assert.match(generatedCss + generatedJs + generatedHtml, new RegExp(className.replace(" ", "[\\s\\S]*")));
  }
});

test("admin ui design system keeps secret handling write-only or show-once", () => {
  assert.match(generatedHtml, /raw-token-template/);
  assert.match(generatedJs, /Token shown once/);
  assert.match(generatedJs, /name="credential" type="password"/);
  assert.match(generatedJs, /name="token" type="password"/);
  assert.match(generatedJs, /name="bearer_token" type="password"/);
  assert.doesNotMatch(generatedJs, /state\.studioConnection\.token\b/);
});
