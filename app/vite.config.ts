import { defineConfig } from "vite";

// Tauri expects a fixed port and no screen clearing in dev mode.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    // WebView2 (Windows), WebKitGTK (Linux) and WKWebView (macOS 10.15+).
    target: ["es2021", "chrome105", "safari14"],
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
  },
});
