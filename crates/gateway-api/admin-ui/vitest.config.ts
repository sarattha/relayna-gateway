import { defineConfig } from "vitest/config";
export default defineConfig({
  test: {
    environment: "jsdom",
    include: ["tests/admin-ui-react.test.tsx"],
    setupFiles: ["tests/admin-ui-react.setup.ts"],
    coverage: {
      provider: "v8",
      include: [
        "crates/gateway-api/admin-ui/src/react/**/*.{ts,tsx}",
        "crates/gateway-api/admin-ui/src/components/**/*.tsx",
      ],
      reporter: ["text", "json-summary", "html"],
      reportsDirectory: "target/admin-ui-coverage",
      thresholds: { lines: 98, statements: 98, branches: 98, functions: 98 },
    },
  },
});
