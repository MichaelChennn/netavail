import { defineConfig } from "vite";

// Served from https://michaelchennn.github.io/netavail/ (architecture.md §5)
export default defineConfig({
  base: "/netavail/",
  worker: { format: "es" },
});
