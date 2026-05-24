import { defineConfig } from "vite";

export default defineConfig({
  root: __dirname,
  base: "/admin-ui/",
  build: {
    outDir: "../src/static/admin-ui",
    emptyOutDir: true,
    cssCodeSplit: false,
    minify: false,
    rollupOptions: {
      output: {
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
