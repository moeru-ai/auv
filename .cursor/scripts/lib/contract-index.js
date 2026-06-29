'use strict';

const fs = require('fs');
const path = require('path');

const CONTRACT_GLOBS = [
  'src/contract.rs',
  'crates/auv-cli-invoke/src/model.rs',
];

const VALUE_RE = /pub\s+const\s+([A-Z0-9_]+)\s*:\s*&str\s*=\s*"([^"]+)"/g;
const ROLE_SUFFIX_RE = /_(API_VERSION|ARTIFACT_ROLE|STATUS|LABEL)$/;

function readContractFiles(repoRoot) {
  const files = [];
  for (const rel of CONTRACT_GLOBS) {
    const abs = path.join(repoRoot, rel);
    if (fs.existsSync(abs)) {
      files.push(abs);
    }
  }
  return files;
}

function buildContractIndex(repoRoot = process.cwd()) {
  const byName = new Map();
  const byValue = new Map();

  for (const file of readContractFiles(repoRoot)) {
    const rel = path.relative(repoRoot, file).replace(/\\/g, '/');
    const text = fs.readFileSync(file, 'utf8');
    let match;
    while ((match = VALUE_RE.exec(text)) !== null) {
      const [, name, value] = match;
      if (!ROLE_SUFFIX_RE.test(name) && !name.endsWith('_ROLE')) {
        continue;
      }
      const entry = { name, value, file: rel };
      byName.set(name, entry);
      const bucket = byValue.get(value) || [];
      bucket.push(entry);
      byValue.set(value, bucket);
    }
  }

  return { byName, byValue, ownerFiles: new Set([...byName.values()].map(v => v.file)) };
}

let cachedIndex = null;
let cachedRoot = null;

function getContractIndex(repoRoot = process.cwd()) {
  const root = path.resolve(repoRoot);
  if (cachedIndex && cachedRoot === root) {
    return cachedIndex;
  }
  cachedIndex = buildContractIndex(root);
  cachedRoot = root;
  return cachedIndex;
}

function resetContractIndexCache() {
  cachedIndex = null;
  cachedRoot = null;
}

module.exports = {
  buildContractIndex,
  getContractIndex,
  resetContractIndexCache,
  ROLE_SUFFIX_RE,
};
