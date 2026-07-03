import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri 期望前端固定端口 1420。
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
});
