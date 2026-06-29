#!/usr/bin/env node
'use strict';

const assert = require('assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');

const repoRoot = path.resolve(__dirname, '..', '..', '..');
const adapter = require(path.join(repoRoot, '.cursor', 'hooks', 'adapter'));
const { resolveCursorEccPluginRoot } = require(path.join(repoRoot, '.cursor', 'scripts', 'lib', 'cursor-ecc-root'));

function runHook(scriptName, input) {
  return spawnSync('node', [path.join(repoRoot, '.cursor', 'hooks', scriptName)], {
    input: JSON.stringify(input),
    encoding: 'utf8',
    cwd: repoRoot,
    env: {
      ...process.env,
      ECC_HOOK_PROFILE: 'standard',
      CURSOR_PROJECT_DIR: repoRoot,
    },
  });
}

function test(name, fn) {
  try {
    fn();
    console.log(`ok ${name}`);
    return true;
  } catch (error) {
    console.error(`fail ${name}: ${error.message}`);
    return false;
  }
}

let passed = 0;
let failed = 0;
function check(name, fn) {
  if (test(name, fn)) passed += 1; else failed += 1;
}

check('resolveCursorEccPluginRoot points at vendored .cursor', () => {
  const pluginRoot = resolveCursorEccPluginRoot({ hostRoot: repoRoot });
  assert.equal(pluginRoot, path.join(repoRoot, '.cursor'));
  assert.ok(fs.existsSync(path.join(pluginRoot, 'scripts', 'hooks', 'post-edit-accumulator.js')));
});

check('adapter.getPluginRoot matches vendored layout', () => {
  const pluginRoot = adapter.getPluginRoot();
  assert.ok(fs.existsSync(path.join(pluginRoot, 'scripts', 'hooks', 'session-start.js')));
});

check('before-shell-execution loads shell-split', () => {
  require(path.join(repoRoot, '.cursor', 'hooks', 'before-shell-execution.js'));
});

check('session-start emits CLAUDE_PLUGIN_ROOT env payload', () => {
  const result = runHook('session-start.js', {
    hook_event_name: 'sessionStart',
    workspace_roots: [repoRoot],
  });
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout.trim());
  assert.ok(payload.env.CLAUDE_PLUGIN_ROOT.includes('.cursor'));
  assert.ok(payload.env.ECC_AGENT_DATA_HOME);
});

check('after-file-edit reaches post-edit accumulator', () => {
  const tmpFile = path.join(os.tmpdir(), `ecc-smoke-${process.pid}.ts`);
  fs.writeFileSync(tmpFile, 'export const x = 1\n');
  fs.writeFileSync(tmpFile, 'export const x = 1\n');
  const result = runHook('after-file-edit.js', {
    hook_event_name: 'afterFileEdit',
    path: tmpFile,
    workspace_roots: [repoRoot],
  });
  assert.equal(result.status, 0, result.stderr);
  const accum = path.join(
    os.tmpdir(),
    `ecc-edited-${require('crypto').createHash('sha1').update(repoRoot).digest('hex').slice(0, 12)}.txt`
  );
  const raw = fs.existsSync(accum) ? fs.readFileSync(accum, 'utf8') : '';
  assert.ok(raw.includes(tmpFile), `accumulator missing edited path: ${accum}`);
  fs.unlinkSync(tmpFile);
});


check('after-file-edit accumulates .rs paths', () => {
  const tmpFile = path.join(os.tmpdir(), `ecc-smoke-rust-${process.pid}.rs`);
  fs.writeFileSync(tmpFile, 'fn x() {}\n');
  const result = runHook('after-file-edit.js', {
    hook_event_name: 'afterFileEdit',
    path: tmpFile,
    workspace_roots: [repoRoot],
  });
  assert.equal(result.status, 0, result.stderr);
  const accum = path.join(
    os.tmpdir(),
    `ecc-edited-${require('crypto').createHash('sha1').update(repoRoot).digest('hex').slice(0, 12)}.txt`
  );
  const raw = fs.existsSync(accum) ? fs.readFileSync(accum, 'utf8') : '';
  assert.ok(raw.includes(tmpFile), `accumulator missing rust path: ${accum}`);
  fs.unlinkSync(tmpFile);
});

check('stop-format-rust runs cargo fmt on accumulated .rs files', () => {
  const smokeDir = path.join(repoRoot, 'target', 'ecc-hook-smoke');
  fs.mkdirSync(smokeDir, { recursive: true });
  const tmpFile = path.join(smokeDir, `fmt-${process.pid}.rs`);
  fs.writeFileSync(tmpFile, 'fn   badly_formatted( )->bool{true}\n');
  const accum = path.join(
    os.tmpdir(),
    `ecc-edited-${require('crypto').createHash('sha1').update(repoRoot).digest('hex').slice(0, 12)}.txt`
  );
  fs.writeFileSync(accum, `${tmpFile}\n`, 'utf8');
  const stopFormatRust = require(path.join(repoRoot, '.cursor', 'scripts', 'hooks', 'stop-format-rust.js'));
  stopFormatRust.run('{}');
  const formatted = fs.readFileSync(tmpFile, 'utf8');
  assert.ok(!/fn\s{2,}/.test(formatted), `expected rustfmt to normalize spacing: ${formatted}`);
  assert.ok(!fs.existsSync(accum), 'rust stop should clear accum when only rust paths were present');
  fs.unlinkSync(tmpFile);
});

check('stop hook runs without throwing', () => {
  const result = runHook('stop.js', {
    hook_event_name: 'stop',
    session_id: `smoke-${process.pid}`,
    transcript_path: path.join(repoRoot, 'missing-transcript.jsonl'),
    cwd: repoRoot,
    last_assistant_message: 'smoke',
  });
  assert.equal(result.status, 0, result.stderr);
});


check('buildProjectContext includes AGENTS.md and excludes codex.md', () => {
  const { buildProjectContext } = require(path.join(repoRoot, '.cursor', 'scripts', 'lib', 'read-project-context'));
  const context = buildProjectContext({ extraStarts: [repoRoot] });
  assert.ok(context.includes('[AGENTS.md'), context.slice(0, 200));
  assert.ok(context.includes('AUV Agent Guide') || context.includes('Project Mission'), context.slice(0, 200));
  assert.ok(!context.includes('[codex.md'), context);
  assert.ok(!context.includes('Codex — AUV review'), context);
});


check('buildProjectContext stays within Cursor inline hook cap', () => {
  const { buildProjectContext, INLINE_CONTEXT_MAX_CHARS } = require(path.join(repoRoot, '.cursor', 'scripts', 'lib', 'read-project-context'));
  const context = buildProjectContext({ extraStarts: [repoRoot] });
  assert.ok(context.length <= INLINE_CONTEXT_MAX_CHARS, `length ${context.length} > ${INLINE_CONTEXT_MAX_CHARS}`);
});

check('inject-project-context emits docs on beforeSubmitPrompt', () => {
  const tmpContributing = path.join(os.tmpdir(), `ecc-smoke-contrib-${process.pid}.md`);
  const tmpCursor = path.join(os.tmpdir(), `ecc-smoke-cursor-${process.pid}.md`);
  fs.writeFileSync(tmpContributing, '# veto\nSlice classification required.\n');
  fs.writeFileSync(tmpCursor, '# cursor\nWork on AUV core.\n');
  const result = spawnSync('node', [path.join(repoRoot, '.cursor', 'hooks', 'inject-project-context.js')], {
    input: JSON.stringify({
      hook_event_name: 'beforeSubmitPrompt',
      prompt: 'smoke test',
      workspace_roots: [repoRoot],
    }),
    encoding: 'utf8',
    cwd: repoRoot,
    env: {
      ...process.env,
      ECC_HOOK_PROFILE: 'standard',
      CURSOR_PROJECT_DIR: repoRoot,
      CONTRIBUTING_LOCAL_PATH: tmpContributing,
      CURSOR_MD_PATH: tmpCursor,
    },
  });
  fs.unlinkSync(tmpContributing);
  fs.unlinkSync(tmpCursor);
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout.trim());
  assert.equal(payload.continue, true);
  const ctx = String(payload.additional_context);
  assert.ok(ctx.includes('Slice classification'), ctx.slice(0, 240));
  assert.ok(ctx.includes('AUV core'), ctx.slice(0, 240));
  assert.ok(ctx.includes('AGENTS.md'), ctx.slice(0, 240));
});

check('pre-compact hook re-injects project context', () => {
  const result = spawnSync('node', [path.join(repoRoot, '.cursor', 'hooks', 'pre-compact.js')], {
    input: JSON.stringify({
      hook_event_name: 'preCompact',
      trigger: 'auto',
      workspace_roots: [repoRoot],
    }),
    encoding: 'utf8',
    cwd: repoRoot,
    env: {
      ...process.env,
      ECC_HOOK_PROFILE: 'standard',
      CURSOR_PROJECT_DIR: repoRoot,
    },
  });
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout.trim());
  assert.ok(String(payload.additional_context).includes('[AGENTS.md'), payload.additional_context?.slice(0, 200));
  assert.ok(String(payload.user_message).includes('compacted'), payload.user_message);
});


check('analyzeStagedSlice blocks paused lane mixed with core', () => {
  const { analyzeStagedSlice } = require('../hooks/pre-bash-staged-slice-veto');
  const violations = analyzeStagedSlice({
    stagedFiles: ['src/candidate_action_decision.rs', 'src/runtime.rs'],
    subject: 'feat(runtime): widen invoke',
  });
  assert.ok(violations.some(v => v.code === 'paused-lane-mix'), JSON.stringify(violations));
});

check('analyzeStagedSlice blocks docs+code without docs subject', () => {
  const { analyzeStagedSlice } = require('../hooks/pre-bash-staged-slice-veto');
  const violations = analyzeStagedSlice({
    stagedFiles: ['docs/ai/references/note.md', 'src/catalog.rs'],
    subject: 'feat(catalog): add command',
  });
  assert.ok(violations.some(v => v.code === 'docs-code-mix'), JSON.stringify(violations));
});

check('commit-gate passes non-commit shell commands', () => {
  const result = spawnSync('node', [path.join(repoRoot, '.cursor', 'hooks', 'before-shell-execution-commit-gate.js')], {
    input: JSON.stringify({ command: 'cargo check' }),
    encoding: 'utf8',
    cwd: repoRoot,
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), JSON.stringify({ command: 'cargo check' }));
});


check('anti-garbage flags env-coupled rust test', () => {
  const { analyzeFileContent } = require('../lib/anti-garbage-heuristics');
  const sample = `#![cfg(test)]
#[test]
fn reads_foreground() {
  let _ = NSWorkspace::sharedWorkspace();
}`;
  const findings = analyzeFileContent('src/foo_test.rs', sample);
  assert.ok(findings.some(f => f.code === 'env-coupled-test'), JSON.stringify(findings));
});

check('anti-garbage flags cross-layer session mix', () => {
  const { analyzeCrossLayer } = require('../lib/anti-garbage-heuristics');
  const findings = analyzeCrossLayer(['src/runtime.rs', 'docs/ai/references/note.md']);
  assert.ok(findings.some(f => f.code === 'cross-layer-mix'), JSON.stringify(findings));
});

check('inject-pending-edit-review emits queue context', () => {
  const os = require('os');
  const queuePath = require('../lib/session-edit-review-queue');
  queuePath.appendReviewEntry({
    filePath: 'src/runtime.rs',
    layers: ['runtime'],
    findings: [{ code: 'fake-refactor-suspect', severity: 'low', message: 'check behavior' }],
  });
  const result = spawnSync('node', [path.join(repoRoot, '.cursor', 'hooks', 'inject-pending-edit-review.js')], {
    input: JSON.stringify({ hook_event_name: 'beforeSubmitPrompt', prompt: 'continue' }),
    encoding: 'utf8',
    cwd: repoRoot,
  });
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout.trim());
  assert.ok(String(payload.additional_context).includes('anti-garbage'), payload.additional_context);
  assert.ok(String(payload.additional_context).includes('src/runtime.rs'), payload.additional_context);
  queuePath.clearQueue();
});

console.log(`cursor-ecc-smoke: ${passed} passed, ${failed} failed`);
process.exit(failed === 0 ? 0 : 1);
