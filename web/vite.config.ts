import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import tailwindcss from "@tailwindcss/vite";

// `npm run dev` proxies the API to a running `claude-hub web --no-open`, logged in with its token.
const target = process.env.CLAUDE_HUB_URL ?? "http://127.0.0.1:7420";

function token(): string {
  const state = process.env.XDG_STATE_HOME ?? join(homedir(), ".local/state");
  try {
    return readFileSync(join(state, "claude-hub/web-token"), "utf8").trim();
  } catch {
    return "";
  }
}

export default defineConfig({
  plugins: [solid(), tailwindcss()],
  build: { target: "es2022", outDir: "dist", emptyOutDir: true, chunkSizeWarningLimit: 900 },
  server: {
    proxy: {
      "/api": {
        target,
        ws: true,
        changeOrigin: true,
        headers: { cookie: `claude_hub=${token()}`, origin: target },
      },
    },
  },
});
