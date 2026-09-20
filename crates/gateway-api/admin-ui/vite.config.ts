import { defineConfig } from "vite";
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { ConsoleShell } from "./src/react/shell.tsx";

export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  plugins: [
    tailwindcss(),
    {
      name: "embedded-react-shell",
      transformIndexHtml: (html) =>
        html.replace(
          "<!--app-shell-->",
          renderToStaticMarkup(createElement(ConsoleShell)),
        ),
    },
  ],
  base: "/admin-ui/",
  build: {
    outDir: "../src/static/admin-ui",
    emptyOutDir: true,
    cssCodeSplit: false,
    minify: false,
    rollupOptions: {
      output: {
        codeSplitting: false,
        entryFileNames: "app.js",
        chunkFileNames: "app.js",
        assetFileNames: (assetInfo) => {
          if (assetInfo.name?.endsWith(".css")) return "app.css";
          return "admin-ui-[name][extname]";
        },
      },
    },
  },
});
