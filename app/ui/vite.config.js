import { resolve } from "path";
import { defineConfig } from "vite";

export default defineConfig({
  build: {
    outDir: "dist",
    rollupOptions: {
      input: {
        overlay: resolve(__dirname, "overlay.html"),
        editor: resolve(__dirname, "editor.html"),
        main: resolve(__dirname, "main.html"),
      },
    },
  },
});
