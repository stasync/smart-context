import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const host = process.env.TAURI_DEV_HOST;

// Tauri settings follow https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [react()],
  // Keep Rust errors visible in the terminal.
  clearScreen: false,
  server: {
    // tauri.conf.json's devUrl expects this exact port.
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    // One HTML entry per Tauri window.
    rolldownOptions: {
      input: {
        settings: fileURLToPath(new URL("settings.html", import.meta.url)),
      },
    },
  },
  test: {
    environment: "jsdom",
  },
});
