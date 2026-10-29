import path from "path"
import tailwindcss from "@tailwindcss/vite"
import { defineConfig } from "vite"
import react from "@vitejs/plugin-react"

export default defineConfig({
  plugins: [
    tailwindcss(),
    react({
      // @ts-expect-error - Vite 8 types are still stabilizing
      reactCompiler: true,
    }),
  ],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    port: 5173,
    proxy: {
      "/api": {
        // Override when the backend isn't on the default port (FYNANCE_PROXY_TARGET=http://localhost:7441 npm run dev).
        target: process.env.FYNANCE_PROXY_TARGET ?? "http://localhost:7433",
        changeOrigin: true,
      },
    },
  },
})
