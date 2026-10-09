import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 1420,
    strictPort: true,
    // Rust changes are handled by Tauri rather than the frontend watcher.
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
  clearScreen: false,
});
