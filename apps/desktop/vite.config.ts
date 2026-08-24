import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes("/node_modules/@uiw/")) {
            return "codemirror-react";
          }
          if (id.includes("/node_modules/@codemirror/view/")) {
            return "codemirror-view";
          }
          if (id.includes("/node_modules/@codemirror/state/")) {
            return "codemirror-state";
          }
          if (
            id.includes("/node_modules/@codemirror/lang-markdown/")
            || id.includes("/node_modules/@lezer/markdown/")
          ) {
            return "codemirror-markdown";
          }
          if (
            id.includes("/node_modules/@codemirror/language/")
            || id.includes("/node_modules/@lezer/")
          ) {
            return "codemirror-language";
          }
          if (id.includes("/node_modules/@codemirror/commands/")) {
            return "codemirror-commands";
          }
          if (id.includes("/node_modules/@codemirror/autocomplete/")) {
            return "codemirror-autocomplete";
          }
          if (id.includes("/node_modules/@codemirror/search/")) {
            return "codemirror-search";
          }
          if (id.includes("/node_modules/@codemirror/lint/")) {
            return "codemirror-lint";
          }
          if (id.includes("/node_modules/@codemirror/")) {
            return "codemirror-core";
          }
          return undefined;
        },
      },
    },
  },
}));
