import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";
import ts from "typescript";

const source = readFileSync(new URL("../crates/gateway-api/admin-ui/src/theme-preference.ts", import.meta.url), "utf8");
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 } }).outputText;
const theme = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString("base64")}`);
const html = readFileSync(new URL("../crates/gateway-api/admin-ui/index.html", import.meta.url), "utf8");
const bootstrap = html.match(/<script>([\s\S]*?)<\/script>/)?.[1];
assert.ok(bootstrap, "the theme bootstrap runs in the head before the static shell");
assert.ok(html.indexOf(bootstrap) < html.indexOf("</head>"));
assert.equal(html.includes("unsafe-inline"), false);

function fixture({ saved = null, dark = false, blocked = false, legacy = false, noMedia = false } = {}) {
  const dom = new JSDOM("<!doctype html><html><body></body></html>", {
    url: "https://gateway.example/admin-ui", runScripts: "outside-only",
  });
  const { window } = dom;
  const listeners = new Set();
  const media = { matches: dark };
  if (!noMedia) {
    media[legacy ? "addListener" : "addEventListener"] = (...args) => listeners.add(args.at(-1));
    media[legacy ? "removeListener" : "removeEventListener"] = (...args) => listeners.delete(args.at(-1));
    window.matchMedia = () => media;
  }
  const storage = window.localStorage;
  if (saved !== null) storage.setItem(theme.THEME_STORAGE_KEY, saved);
  if (blocked) Object.defineProperty(window, "localStorage", { get() { throw new Error("Storage blocked"); } });
  const originals = { window: globalThis.window, document: globalThis.document, CustomEvent: globalThis.CustomEvent };
  Object.assign(globalThis, { window, document: window.document, CustomEvent: window.CustomEvent });
  const events = [];
  window.document.addEventListener(theme.THEME_CHANGE_EVENT, event => events.push(event.detail));
  let dispose;
  return {
    window, storage, events, listeners,
    bootstrap() { window.eval(bootstrap); },
    start() { dispose = theme.initializeThemePreference(); },
    stop() { dispose?.(); },
    system(dark) {
      media.matches = dark;
      for (const listener of [...listeners]) listener({ matches: dark });
    },
    sync(key, newValue, storageArea = storage) {
      window.dispatchEvent(new window.StorageEvent("storage", { key, newValue, storageArea }));
    },
    close() { dispose?.(); Object.assign(globalThis, originals); window.close(); },
  };
}

// Bootstrap and the live controller must resolve every persisted value identically.
for (const saved of [null, "system", "light", "dark", "invalid", "DARK", '{"theme":"dark"}']) {
  for (const dark of [false, true]) {
    const f = fixture({ saved, dark });
    try {
      f.bootstrap();
      const expectedPreference = ["light", "dark"].includes(saved) ? saved : "system";
      const expectedTheme = expectedPreference === "system" ? (dark ? "dark" : "light") : expectedPreference;
      assert.equal(document.documentElement.dataset.theme, expectedTheme);
      f.start();
      assert.equal(theme.getThemePreference(), expectedPreference);
      assert.equal(document.documentElement.dataset.theme, expectedTheme);
      assert.equal(f.storage.getItem(theme.THEME_STORAGE_KEY), saved, "reading never writes a preference");
    } finally { f.close(); }
  }
}

for (const legacy of [false, true]) {
  const f = fixture({ legacy });
  try {
    f.start();
    f.events.length = 0;
    assert.equal(f.listeners.size, 1);
    f.system(true);
    assert.deepEqual(f.events, [{ theme: "dark", preference: "system" }]);
    f.system(true);
    assert.equal(f.events.length, 1, "unchanged OS notifications do not duplicate events");
    theme.setThemePreference("dark");
    assert.deepEqual(f.events.at(-1), { theme: "dark", preference: "dark" });
    assert.equal(f.events.length, 2, "preference-only changes notify both selectors once");
    assert.equal(f.listeners.size, 0, "explicit preferences stop listening to the OS");
    f.system(false);
    assert.equal(document.documentElement.dataset.theme, "dark");
    theme.setThemePreference("system");
    assert.equal(document.documentElement.dataset.theme, "light");
    assert.equal(f.listeners.size, 1);
    const beforeInvalid = f.events.length;
    theme.setThemePreference("invalid");
    assert.equal(theme.getThemePreference(), "system");
    assert.equal(f.events.length, beforeInvalid);
    f.sync("unrelated", "dark");
    f.sync(theme.THEME_STORAGE_KEY, "dark", f.window.sessionStorage);
    assert.equal(theme.getThemePreference(), "system");
    f.sync(theme.THEME_STORAGE_KEY, "dark");
    assert.equal(theme.getThemePreference(), "dark");
    assert.equal(document.documentElement.dataset.theme, "dark");
    assert.equal(f.storage.getItem(theme.THEME_STORAGE_KEY), "system", "storage sync never echoes writes");
    f.sync(theme.THEME_STORAGE_KEY, "invalid");
    assert.equal(theme.getThemePreference(), "system");
    f.sync(theme.THEME_STORAGE_KEY, "dark");
    f.sync(theme.THEME_STORAGE_KEY, null);
    assert.equal(theme.getThemePreference(), "system", "removal restores System");
    f.sync(theme.THEME_STORAGE_KEY, "dark");
    f.sync(null, null);
    assert.equal(theme.getThemePreference(), "system", "clearing storage restores System");
    const beforeTeardown = f.events.length;
    f.stop();
    f.stop();
    assert.equal(f.listeners.size, 0);
    f.system(true);
    f.sync(theme.THEME_STORAGE_KEY, "dark");
    theme.setThemePreference("dark");
    assert.equal(f.events.length, beforeTeardown, "teardown detaches OS and storage listeners");
  } finally { f.close(); }
}

for (const noMedia of [false, true]) {
  const f = fixture({ blocked: true, dark: true, noMedia });
  try {
    f.bootstrap();
    assert.equal(document.documentElement.dataset.theme, noMedia ? "light" : "dark");
    f.start();
    assert.equal(theme.getThemePreference(), "system");
    theme.setThemePreference("light");
    assert.equal(document.documentElement.dataset.theme, "light");
    theme.setThemePreference("dark");
    assert.equal(document.documentElement.dataset.theme, "dark");
    assert.equal(theme.getThemePreference(), "dark", "storage failures preserve in-memory selection");
  } finally { f.close(); }
}

console.log("ok - early theme resolution, enum preferences, storage failures/sync, OS changes, legacy media APIs and teardown");
