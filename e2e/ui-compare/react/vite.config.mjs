// One page per screen, so a screen loads only the code it uses. The chunks the screens
// share (React itself, the styles) are split out by the bundler.
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const here = dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { "@": resolve(here, "src") },
  },
  build: {
    target: "es2022",
    sourcemap: false,
    cssCodeSplit: true,
    reportCompressedSize: false,
    chunkSizeWarningLimit: 1000,
    rollupOptions: {
      input: {
        table: resolve(here, "table.html"),
        select: resolve(here, "select.html"),
        dates: resolve(here, "dates.html"),
        tabs: resolve(here, "tabs.html"),
      },
    },
  },
});
