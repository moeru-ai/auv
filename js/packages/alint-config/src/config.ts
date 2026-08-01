import { createApeiraAdapter } from "@alint-js/agent-apeira";
import { defineConfig } from "@alint-js/plugin";
import auv from "./plugins/auv";

export default defineConfig([
  {
    name: "auv/rust",
    directories: ["crates/*"],
    files: ["**/*.rs"],
    language: "plaintext",
    agent: createApeiraAdapter(),
    plugins: {
      rust: auv,
    },
    rules: {
      "rust/no-vacant-control-boundary": "warn",
      "rust/no-private-schema-toolkit": "warn",
      "rust/no-unearned-function-boundary": "warn",
      "rust/prefer-established-foundation": "warn",
    },
  },
  {
    name: "auv/rust-test-contracts",
    files: ["**/{src,tests,examples}/**/*.rs"],
    language: "plaintext",
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
    name: "auv/side-by-side-rust-unit-tests",
    files: [
      "**/{src,examples}/**/*.rs",
    ],
    language: "plaintext",
    agent: createApeiraAdapter(),
    plugins: {
      rust: auv,
    },
    rules: {
      "rust/require-side-by-side-unit-tests": "error",
    },
  },
  {
    name: "auv/non-runtime-test-ownership",
    files: [
      "**/{src,tests,examples}/**/*.rs",
    ],
    language: "plaintext",
    agent: createApeiraAdapter(),
    plugins: {
      rust: auv,
    },
    rules: {
      "rust/restrict-non-runtime-unit-tests": "error",
    },
  },
]);
