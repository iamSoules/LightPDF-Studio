import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  base: "./",
  plugins: [react(), tailwindcss()],
  build: {
    target: "es2020",
    chunkSizeWarningLimit: 4000,
    rollupOptions: {
      output: {
        manualChunks: {
          pdfjs: ["pdfjs-dist/legacy/build/pdf.mjs"],
          pdflib: ["@cantoo/pdf-lib"],
        },
      },
    },
  },
  worker: { format: "es" },
});
