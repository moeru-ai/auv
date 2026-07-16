import { createApeiraAdapter } from "@alint-js/agent-apeira";
import { defineConfig } from "@alint-js/plugin";
import auv from "./plugins/auv";

export default defineConfig([
  {
    name: "auv/rust",
    directories: ["crates/*"],
    files: ["**/*.rs"],
    language: "text/plain",
    agent: createApeiraAdapter(),
    plugins: {
      rust: auv,
    },
    rules: {
      "rust/no-vacant-control-boundary": "warn",
      "rust/prefer-established-foundation": "warn",
    },
  },
  {
    name: "auv/repo-text-and-scripts",
    files: ["**/*.{toml,md,yml,yaml,json,js,mjs,cjs,ts,tsx,mts,cts,vue}"],
  },
]);
