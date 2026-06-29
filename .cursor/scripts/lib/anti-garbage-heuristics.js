'use strict';

const fs = require('fs');
const path = require('path');

const LAYER_RULES = [
  { layer: 'runtime', test: p => /^(src\/runtime\.rs|src\/invoke|crates\/[^/]*runtime)/.test(p) },
  { layer: 'read', test: p => /run_read|inspect|inspection/.test(p) },
  { layer: 'cli', test: p => /^(src\/cli|crates\/auv-cli)/.test(p) },
  { layer: 'proto', test: p => /\.proto$|\/proto\//.test(p) },
  { layer: 'docs', test: p => /^docs\//.test(p) || (p.endsWith('.md') && !p.startsWith('.cursor/')) },
  { layer: 'hooks', test: p => /^\.cursor\//.test(p) },
  { layer: 'paused', test: p => /candidate_action_(decision|command)/.test(p) },
  { layer: 'driver', test: p => /^crates\/auv-driver/.test(p) },
  { layer: 'test', test: p => /\/tests?\//.test(p) || /_test\.rs$/.test(p) },
];

const ENV_TEST_SIGNALS = [
  { pattern: /\bNSWorkspace\b/, label: 'NSWorkspace (desktop state)' },
  { pattern: /\bAXUIElement\b/, label: 'AXUIElement (accessibility/desktop)' },
  { pattern: /\bCGWindow\b/, label: 'CGWindow (window server)' },
  { pattern: /\bforeground(_app| window)?\b/i, label: 'foreground app/window' },
  { pattern: /\breqwest::/, label: 'reqwest network call' },
  { pattern: /\bstd::env::var\(/, label: 'environment variable read' },
  { pattern: /\blocalhost:\d+/, label: 'localhost network endpoint' },
];

const FORWARD_BODY_PATTERNS = [
  {
    code: 'fake-abstraction-forward-fn',
    pattern:
      /(?:pub\s+)?(?:async\s+)?fn\s+\w+\([^)]*\)(?:\s*->[^{]+)?\{\s*(?:[\w:]+::)?[\w.]+\([^;{}]*\)\s*;?\s*\}/,
    message: 'Function body looks like a single forward call — inline or own a real boundary.',
  },
  {
    code: 'fake-abstraction-delegate-struct',
    pattern: /struct\s+\w+Deps\b|struct\s+Dependencies\b/,
    message: 'New Dependencies/Deps bag — verify it adds policy, not pass-through.',
  },
];

function classifyLayer(filePath) {
  const normalized = String(filePath || '').replace(/\\/g, '/');
  return LAYER_RULES.filter(rule => rule.test(normalized)).map(rule => rule.layer);
}

function analyzeCrossLayer(sessionPaths) {
  const layers = new Set();
  for (const filePath of sessionPaths) {
    for (const layer of classifyLayer(filePath)) {
      layers.add(layer);
    }
  }

  const riskyPairs = [
    ['runtime', 'docs'],
    ['runtime', 'proto'],
    ['read', 'docs'],
    ['cli', 'proto'],
    ['paused', 'runtime'],
    ['driver', 'docs'],
  ];

  const violations = [];
  for (const [a, b] of riskyPairs) {
    if (layers.has(a) && layers.has(b)) {
      violations.push({
        code: 'cross-layer-mix',
        severity: 'high',
        message: `Session edits span ${a} and ${b}. Split the slice before review gets opaque.`,
        detail: `layers: ${[...layers].join(', ')}`,
      });
      break;
    }
  }

  if (layers.size >= 4 && violations.length === 0) {
    violations.push({
      code: 'cross-layer-wide',
      severity: 'medium',
      message: `Session edits touch ${layers.size} layers (${[...layers].join(', ')}). Confirm this is one named slice.`,
    });
  }

  return violations;
}

function analyzeFileContent(filePath, content) {
  const findings = [];
  const text = String(content || '');
  const rel = String(filePath || '').replace(/\\/g, '/');
  const isRust = rel.endsWith('.rs');
  const isJs = /\.(js|jsx|ts|tsx)$/.test(rel);

  for (const rule of FORWARD_BODY_PATTERNS) {
    if (rule.pattern.test(text)) {
      findings.push({
        code: rule.code,
        severity: 'medium',
        message: rule.message,
      });
    }
  }

  if (isJs) {
    const jsForward = /export\s+(?:async\s+)?function\s+\w+\([^)]*\)\s*\{\s*return\s+[\w.]+\([^;{}]*\)\s*;?\s*\}/;
    if (jsForward.test(text)) {
      findings.push({
        code: 'fake-abstraction-js-forward',
        severity: 'medium',
        message: 'Exported function only forwards to another call — inline unless reused twice with real policy.',
      });
    }
  }

  if (isRust && /trait\s+\w+/.test(text) && /fn\s+\w+[^}]*\{\s*self\.\w+\.\w+\(/.test(text)) {
    findings.push({
      code: 'fake-abstraction-trait-forward',
      severity: 'medium',
      message: 'Trait method appears to forward to an inner field — avoid pass-through traits.',
    });
  }

  const lineCount = text.split('\n').length;
  if (lineCount > 0 && lineCount <= 45 && /helper|util|wrapper/i.test(path.basename(rel))) {
    const fnBodies = text.match(/\{[^{}]{0,120}\}/g) || [];
    if (fnBodies.length <= 2 && fnBodies.every(body => body.split(';').length <= 2)) {
      findings.push({
        code: 'premature-extraction',
        severity: 'medium',
        message: 'Small helper/wrapper file with tiny bodies — confirm extraction is reused and owns real policy.',
      });
    }
  }

  if (isRust && /#\[test\]|#\[tokio::test\]/.test(text)) {
    const hasLiveLabel = /\b(live|integration|desktop)_/i.test(text) || /#\[ignore\]/.test(text);
    if (!hasLiveLabel) {
      for (const signal of ENV_TEST_SIGNALS) {
        if (signal.pattern.test(text)) {
          findings.push({
            code: 'env-coupled-test',
            severity: 'high',
            message: `Test uses ${signal.label} without live/integration label or #[ignore].`,
          });
          break;
        }
      }
    }
  }

  const logicSignals = (text.match(/\b(match|if|else|return Err|assert_eq!|expect\()/g) || []).length;
  const renameSignals = (text.match(/\bpub\s+fn\s+\w+|\bmod\s+\w+/g) || []).length;
  if (logicSignals >= 6 && renameSignals >= 3) {
    findings.push({
      code: 'fake-refactor-suspect',
      severity: 'low',
      message: 'File mixes structural churn with logic edits — confirm behavior is unchanged or add regression tests.',
    });
  }

  return findings;
}

function analyzeEditDelta(edits) {
  const findings = [];
  const joined = (edits || [])
    .map(edit => `${edit?.old_string || ''}\n${edit?.new_string || ''}`)
    .join('\n');

  if (!joined.trim()) {
    return findings;
  }

  const logicDelta = (joined.match(/\b(if|match|return|assert|expect|Err\(|unwrap\()/g) || []).length >= 4;
  const renameDelta = /=>|rename|mv |move /.test(joined);

  if (logicDelta && renameDelta) {
    findings.push({
      code: 'rename-behavior-delta',
      severity: 'medium',
      message: 'This edit batch mixes rename/move strings with logic changes — split refactor vs behavior.',
    });
  }

  return findings;
}

function readFileSafe(filePath) {
  try {
    return fs.readFileSync(path.resolve(filePath), 'utf8');
  } catch {
    return '';
  }
}

function reviewEditedFile({ filePath, edits, sessionPaths }) {
  const content = readFileSafe(filePath);
  const findings = [
    ...analyzeFileContent(filePath, content),
    ...analyzeEditDelta(edits),
    ...analyzeCrossLayer([...sessionPaths, filePath]),
  ];

  const unique = [];
  const seen = new Set();
  for (const finding of findings) {
    const key = `${finding.code}:${finding.message}`;
    if (seen.has(key)) continue;
    seen.add(key);
    unique.push(finding);
  }

  return {
    filePath,
    layers: classifyLayer(filePath),
    findings: unique,
  };
}

module.exports = {
  LAYER_RULES,
  classifyLayer,
  analyzeCrossLayer,
  analyzeFileContent,
  analyzeEditDelta,
  reviewEditedFile,
  readFileSafe,
};
