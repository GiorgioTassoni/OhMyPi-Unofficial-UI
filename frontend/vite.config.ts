import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";

// Tauri drives this dev server, so the port is fixed (`tauri.conf.json` points at
// it) and a port collision must fail loudly rather than silently move.
export default defineConfig({
  plugins: [vue(), tailwindcss()],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
  },
  build: {
    // The webview is whatever the platform ships, so target a modern baseline
    // rather than transpiling for browsers we will never run in.
    target: "es2022",
    sourcemap: true,
  },
});
