import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri drives this dev server, so the port is fixed and failing is better than
// silently moving to 1421 where the Rust side is not looking.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // The Rust half has its own rebuild loop; watching it here just causes
      // the page to reload in the middle of a cargo build.
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    target: "es2022",
    sourcemap: true,
  },
});
