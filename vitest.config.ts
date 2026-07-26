import { defineConfig } from "vitest/config";
import path from "path";

// Deliberately separate from vite.config.ts: tests don't need the React or
// Tailwind plugins, and the app config is async/multi-page which vitest
// doesn't care about. Only the "@" alias must stay in sync.
export default defineConfig({
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
