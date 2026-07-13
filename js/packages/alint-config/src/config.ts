import { defineConfig } from "@alint-js/core";
import auv from "./plugins/auv";

export default defineConfig([
  {
    name: "auv/rust",
    files: ["**/*.rs"],
    language: "text/plain",
    plugins: {
      rust: auv,
    },
    rules: {
      "rust/todo": "warn",
    },
  },
  {
    name: "auv/repo-text-and-scripts",
    files: ["**/*.{toml,md,yml,yaml,json,js,mjs,cjs,ts,tsx,mts,cts,vue}"],
  },
]);
