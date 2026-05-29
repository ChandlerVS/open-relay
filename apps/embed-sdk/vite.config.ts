import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Library mode: emit a single IIFE that hosts can drop into a <script> tag.
// React and ReactDOM are bundled in (no peer-dep on the host page).
export default defineConfig({
  plugins: [react()],
  define: {
    "process.env.NODE_ENV": JSON.stringify("production"),
  },
  build: {
    lib: {
      entry: "src/index.ts",
      name: "OpenRelay",
      formats: ["iife"],
      fileName: () => "open-relay.js",
    },
    cssCodeSplit: false,
  },
});
