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
      "rust/no-private-schema-toolkit": "warn",
      "rust/prefer-established-foundation": "warn",
    },
  },
  {
    name: "auv/rust-test-contracts",
    files: ["src/**/*.rs", "tests/**/*.rs", "crates/*/src/**/*.rs", "crates/*/tests/**/*.rs"],
    language: "text/plain",
    agent: createApeiraAdapter(),
    plugins: {
      rust: auv,
    },
    rules: {
      "rust/no-mod-names-checks-in-tests": "error",
      "rust/no-source-files-compare-in-tests": "error",
    },
  },
  {
    name: "auv/app-integration-directories",
    directories: [
      "crates/auv-apple-music",
      "crates/auv-apple-notes",
      "crates/auv-apple-textedit",
      "crates/auv-gnome-control-center",
      "crates/auv-netease-music",
      "crates/auv-qqmusic",
    ],
    agent: createApeiraAdapter(),
    plugins: {
      rust: auv,
    },
    rules: {
      "rust/require-platform-scoped-app-integration": "warn",
    },
  },
  {
    name: "auv/repo-text-and-scripts",
    files: ["**/*.{toml,md,yml,yaml,json,js,mjs,cjs,ts,tsx,mts,cts,vue}"],
  },
]);
