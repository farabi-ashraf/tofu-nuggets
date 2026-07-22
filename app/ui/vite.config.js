import { resolve } from "path";
import { defineConfig } from "vite";

export default defineConfig({
  build: {
    outDir: "dist",
    rollupOptions: {
      input: {
        overlay: resolve(__dirname, "overlay.html"),
        badges: resolve(__dirname, "badges.html"),
        editor: resolve(__dirname, "editor.html"),
        main: resolve(__dirname, "main.html"),
        settings: resolve(__dirname, "settings.html"),
      },
    },
  },
});
