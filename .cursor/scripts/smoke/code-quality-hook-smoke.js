#!/usr/bin/env node
'use strict';

const assert = require('assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');

const repoRoot = path.resolve(__dirname, '..', '..', '..');
const heuristics = require(path.join(repoRoot, '.cursor/scripts/lib/code-quality-heuristics'));
const queue = require(path.join(repoRoot, '.cursor/scripts/lib/session-code-quality-queue'));
const { run: runPostEdit } = require(path.join(repoRoot, '.cursor/scripts/hooks/post-edit-code-quality-review'));
const { run: runStop } = require(path.join(repoRoot, '.cursor/scripts/hooks/stop-code-quality-review'));

let passed = 0;
let failed = 0;

function check(name, fn) {
  try {
    fn();
    console.log(`ok ${name}`);
    passed += 1;
  } catch (error) {
    console.error(`fail ${name}: ${error.message}`);
    failed += 1;
  }
}

const FIXTURE_SAMPLE = `#![cfg(test)]
use super::*;

#[test]
fn test_a() {
  let root = tempdir().unwrap();
  let store = LocalStore::new(root.path().to_path_buf()).unwrap();
  let run = dummy_run("run-a");
  let _ = stage_json_artifact(&store, &run, "role-a", &serde_json::json!({}));
  persist_run_with_operation_result(&store, &run);
}

#[test]
fn test_b() {
  let root = tempdir().unwrap();
  let store = LocalStore::new(root.path().to_path_buf()).unwrap();
  let run = dummy_run("run-b");
  let _ = stage_json_artifact(&store, &run, "role-b", &serde_json::json!({}));
  persist_run_with_operation_result(&store, &run);
}
`;

const FAT_HANDLER_SAMPLE = `pub async fn invoke(&self, req: Request) -> Result<Response> {
  let parsed = parse_request(&req)?;
  let routed = dispatch_command(parsed)?;
  let cached = self.cache.get(&routed.id).unwrap_or_default();
  let merged = join_results(cached, routed)?;
  let saved = persist_summary(&merged)?;
  let body = format!("{}", serde_json::to_string(&saved)?);
  Ok(Response::new(body))
}
`;

check('fixture duplication in same file is medium', () => {
  const findings = heuristics.analyzeFixtureDuplication(
    'src/api/session_service/summary.rs',
    FIXTURE_SAMPLE,
    [],
    repoRoot,
  );
  const hit = findings.find(f => f.code === 'fixture-duplication');
  assert.ok(hit, JSON.stringify(findings));
  assert.equal(hit.severity, 'medium');
  assert.ok(hit.evidence.includes('test_a'));
  assert.ok(hit.suggested_action.includes('staging helper'));
});

check('fat entrypoint with many stages is high', () => {
  const findings = heuristics.analyzeEntrypointCreep(
    'src/api/session_service/handler.rs',
    FAT_HANDLER_SAMPLE,
    [],
    repoRoot,
  );
  const hit = findings.find(f => f.code === 'entrypoint-responsibility-creep' && f.severity === 'high');
  assert.ok(hit, JSON.stringify(findings));
  assert.ok(hit.evidence.includes('invoke'));
});

check('duplicate contract constant outside owner is flagged', () => {
  const sample = 'const OPERATION_SUMMARY_ARTIFACT_ROLE: &str = "operation-summary";\n';
  const findings = heuristics.analyzeDuplicateContract(
    'src/api/session_service/summary_store.rs',
    sample,
    [{ new_string: sample }],
    repoRoot,
  );
  const hit = findings.find(f => f.code === 'duplicate-contract-ownership');
  assert.ok(hit, JSON.stringify(findings));
  assert.ok(['medium', 'high'].includes(hit.severity));
});

check('docs-only session suppresses high findings', () => {
  const review = heuristics.reviewEditedFile({
    filePath: path.join(repoRoot, 'docs/ai/references/note.md'),
    edits: [{ new_string: '# handoff\nNOTICE: full narrative\n' }],
    sessionPaths: ['docs/ai/references/note.md', 'docs/README.md'],
    repoRoot,
  });
  assert.equal(review.docsOnly, true);
  assert.ok(!review.findings.some(f => f.severity === 'high'), JSON.stringify(review.findings));
});

check('rough temp persistence warns on staging path', () => {
  const sample = 'let path = store_root.join(".staging-summary.json");\nstd::fs::write(&path, bytes)?;\n';
  const findings = heuristics.analyzeRoughTempPersistence(
    'src/api/session_service/summary_store.rs',
    sample,
    [{ new_string: sample }],
    repoRoot,
  );
  const hit = findings.find(f => f.code === 'rough-temp-persistence');
  assert.ok(hit, JSON.stringify(findings));
});

check('docs-cleaner-than-code only fires with structural signals', () => {
  const structural = [{
    code: 'fixture-duplication',
    severity: 'medium',
    file: 'src/foo_test.rs',
    evidence: 'dup',
    why_it_matters: 'x',
    suggested_action: 'y',
    message: 'dup',
  }];
  const findings = heuristics.analyzeDocsCleanerThanCode(
    'src/foo_test.rs',
    '//! module docs\n',
    [{ new_string: '// NOTICE: handoff complete\n// TODO(slice): follow-up\n' }],
    structural,
    repoRoot,
  );
  assert.equal(findings.length, 1);
  assert.equal(findings[0].severity, 'medium');
  assert.equal(findings[0].code, 'docs-cleaner-than-code');

  const none = heuristics.analyzeDocsCleanerThanCode(
    'src/foo_test.rs',
    '',
    [{ new_string: '// NOTICE only\n' }],
    [],
    repoRoot,
  );
  assert.equal(none.length, 0);
});

check('queue dedups same edit fingerprint', () => {
  queue.clearQueue();
  const entry = {
    filePath: 'src/api/session_service/handler.rs',
    findings: [{
      code: 'entrypoint-responsibility-creep',
      severity: 'medium',
      file: 'src/api/session_service/handler.rs',
      evidence: 'invoke added cache + persist',
      why_it_matters: 'creep',
      suggested_action: 'extract',
      message: 'invoke added cache + persist',
    }],
  };
  queue.upsertReviewEntry(entry, { contentHash: 'deadbeef', source: 'afterFileEdit' });
  queue.upsertReviewEntry(entry, { contentHash: 'deadbeef', source: 'postToolUse' });
  const state = queue.readQueue();
  assert.equal(state.entries.length, 1, JSON.stringify(state.entries));
  queue.clearQueue();
});

check('inject hook emits queued context', () => {
  queue.upsertReviewEntry({
    filePath: 'src/api/session_service/handler.rs',
    findings: [{
      code: 'entrypoint-responsibility-creep',
      severity: 'medium',
      file: 'src/api/session_service/handler.rs',
      evidence: 'invoke added cache + persist',
      why_it_matters: 'creep',
      suggested_action: 'extract helper',
      message: 'invoke added cache + persist',
    }],
  });
  const result = spawnSync('node', [path.join(repoRoot, '.cursor/hooks/inject-pending-code-quality-review.js')], {
    input: JSON.stringify({ hook_event_name: 'beforeSubmitPrompt', prompt: 'continue' }),
    encoding: 'utf8',
    cwd: repoRoot,
  });
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout.trim());
  assert.ok(String(payload.additional_context).includes('code-quality'), payload.additional_context);
  assert.ok(String(payload.additional_context).includes('entrypoint-responsibility-creep'), payload.additional_context);
  queue.clearQueue();
});

check('stop hook requests followup on high findings', () => {
  queue.upsertReviewEntry({
    filePath: 'src/api/session_service/handler.rs',
    findings: [{
      code: 'entrypoint-responsibility-creep',
      severity: 'high',
      file: 'src/api/session_service/handler.rs',
      evidence: 'invoke() 5 stages',
      why_it_matters: 'fat entry',
      suggested_action: 'split',
      message: 'invoke() 5 stages',
    }],
  });
  const result = runStop(JSON.stringify({ loop_count: 0 }));
  const payload = JSON.parse(result.stdout);
  assert.ok(payload.followup_message, result.stdout);
  assert.ok(payload.followup_message.includes('Code-quality review'), payload.followup_message);
});

console.log(`code-quality-hook-smoke: ${passed} passed, ${failed} failed`);
process.exit(failed === 0 ? 0 : 1);
