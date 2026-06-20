import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
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
const designSystemShim = read("crates/gateway-api/admin-ui/src/design-system.ts");
const designSystemIndex = read("crates/gateway-api/admin-ui/src/design-system/index.ts");
const viewMeta = read("crates/gateway-api/admin-ui/src/design-system/view-meta.ts");
const components = read("crates/gateway-api/admin-ui/src/design-system/components.ts");
const templates = read("crates/gateway-api/admin-ui/src/design-system/templates.ts");
const tokens = read("crates/gateway-api/admin-ui/src/design-system/tokens.css");
const designSystemReadme = read("crates/gateway-api/admin-ui/src/design-system/README.md");
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
  for (const view of ["overview", "providers", "services", "routes", "projects", "keys", "guardrails", "audit", "usage", "health", "settings"]) {
    assert.match(sourceHtml, new RegExp(`data-view="${view}"`));
    assert.match(viewMeta, new RegExp(`${view}: \\{`));
  }
  for (const domain of ["Monitor", "Discover", "Govern"]) {
    assert.match(sourceHtml, new RegExp(`<span>${domain}<\\/span>`));
    assert.match(viewMeta, new RegExp(`domain: "${domain}"`));
  }
});

test("admin ui design system is split into reusable source modules", () => {
  for (const file of ["index.ts", "tokens.css", "view-meta.ts", "components.ts", "templates.ts", "README.md"]) {
    assert.ok(existsSync(join(root, "crates/gateway-api/admin-ui/src/design-system", file)), `missing ${file}`);
  }
  assert.match(designSystemShim, /export \* from "\.\/design-system\/index"/);
  assert.match(designSystemIndex, /export \* from "\.\/components"/);
  assert.match(designSystemIndex, /export \* from "\.\/templates"/);
  assert.match(designSystemIndex, /export \* from "\.\/view-meta"/);
  for (const helper of ["function badge", "function panel", "function metricTile", "function emptyState", "function tableWrap", "function actionGroup"]) {
    assert.match(components, new RegExp(helper));
  }
  for (const helper of ["function dashboardTemplate", "function auditLogTemplate", "function analyticsTemplate", "function importDiffTemplate"]) {
    assert.match(templates, new RegExp(helper));
  }
  assert.match(designSystemReadme, /Preserve security invariants/);
});

test("admin ui design system exposes 2.0 tokens and operator-console components", () => {
  for (const token of ["--rg-color-bg", "--rg-color-surface", "--rg-color-accent", "--rg-status-good", "--rg-status-bad", "--rg-focus-ring"]) {
    assert.match(tokens, new RegExp(token));
    assert.match(generatedCss, new RegExp(token));
  }
  assert.match(sourceCss, /@import "\.\/design-system\/tokens\.css"/);
  for (const className of ["nav-group", "metric-strip", "panel-heading", "badge good", "modal-backdrop", "table-wrap", "form-grid"]) {
    assert.match(generatedCss + generatedJs + generatedHtml, new RegExp(className.replace(" ", "[\\s\\S]*")));
  }
});

test("generated admin ui uses design-system helpers and exposes release controls", () => {
  for (const marker of [
    "function badge",
    "function panel",
    "function tableWrap",
    "function auditLogTemplate",
    "function importDiffTemplate",
    "Audit filters",
    "Usage export JSON",
    "Usage export CSV",
    "Manage provider health state",
    "Sync selected",
    "Security and release posture",
    "version-indicator",
  ]) {
    assert.match(generatedJs + generatedHtml, new RegExp(marker));
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
