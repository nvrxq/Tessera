import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    target: "es2022",
    minify: "esbuild",
    sourcemap: false,
    rollupOptions: {
      output: {
        // Pin vendor libraries into stable chunks. Splitting on
        // node_modules boundaries means releases that only change app
        // code don't bust the cached vendor JS — disk cache hit-rate
        // jumps and cold start after an update is noticeably faster.
        manualChunks: (id) => {
          if (id.includes("node_modules/solid-js")) return "vendor-solid";
          if (id.includes("node_modules/@tauri-apps")) return "vendor-tauri";
          if (id.includes("node_modules/@fontsource")) return "vendor-fonts";
        },
      },
    },
  },
});
