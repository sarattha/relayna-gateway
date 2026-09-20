import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";
afterEach(() => cleanup());
if (!globalThis.CSS) Object.defineProperty(globalThis, "CSS", { value: {} });
CSS.escape ||= (value) =>
  String(value).replace(/[^a-zA-Z0-9_-]/g, (char) => `\\${char}`);
Element.prototype.scrollIntoView ||= () => {};
globalThis.ResizeObserver ||= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
