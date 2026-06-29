'use strict';

const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');

const MAX_ENTRIES = 12;

function sessionKey() {
  const raw =
    process.env.CLAUDE_SESSION_ID ||
    process.env.CURSOR_CONVERSATION_ID ||
    crypto.createHash('sha1').update(process.cwd()).digest('hex').slice(0, 12);
  return raw.replace(/[^a-zA-Z0-9_-]/g, '_').slice(0, 64);
}

function queuePath() {
  return path.join(os.tmpdir(), `ecc-code-quality-queue-${sessionKey()}.json`);
}

function readQueue() {
  try {
    const parsed = JSON.parse(fs.readFileSync(queuePath(), 'utf8'));
    return {
      entries: Array.isArray(parsed.entries) ? parsed.entries : [],
      allPaths: Array.isArray(parsed.allPaths) ? parsed.allPaths : [],
    };
  } catch {
    return { entries: [], allPaths: [] };
  }
}

function writeQueue(queue) {
  const file = queuePath();
  const entries = queue.entries.slice(-MAX_ENTRIES);
  const allPaths = [...new Set(queue.allPaths.map(p => String(p || '').trim()).filter(Boolean))];
  if (entries.length === 0 && allPaths.length === 0) {
    try {
      fs.unlinkSync(file);
    } catch {
      /* best-effort */
    }
    return;
  }
  fs.writeFileSync(file, JSON.stringify({ entries, allPaths }, null, 2), 'utf8');
}

function findingFingerprint(finding) {
  return [finding.code, finding.file, finding.evidence, finding.severity].join('|');
}

function reviewFingerprint(entry, meta = {}) {
  const codes = (entry.findings || []).map(f => findingFingerprint(f)).sort().join(';');
  const payload = [
    String(entry.filePath || ''),
    codes,
    String(meta.contentHash || ''),
    String(meta.toolUseId || ''),
  ].join('|');
  return crypto.createHash('sha1').update(payload).digest('hex').slice(0, 16);
}

function upsertReviewEntry(entry, meta = {}) {
  const queue = readQueue();
  const fingerprint = reviewFingerprint(entry, meta);
  const duplicate = queue.entries.some(
    existing => existing.filePath === entry.filePath && existing.fingerprint === fingerprint,
  );
  if (duplicate) {
    return queue;
  }

  queue.entries.push({
    ...entry,
    fingerprint,
    source: meta.source || '',
    timestamp: new Date().toISOString(),
  });
  if (entry.filePath) {
    queue.allPaths.push(entry.filePath);
  }
  writeQueue(queue);
  return queue;
}

function consumePendingContext(maxChars = 3200) {
  const queue = readQueue();
  if (queue.entries.length === 0) {
    return { context: '', queue, hadPending: false };
  }

  const lines = [
    '[AUV code-quality edit review]',
    'Before writing more code, re-check the latest edit(s) for maintainability drift:',
    '- fixture-duplication: copied staging/dummy_run/setup blocks that will multi-sync on shape change',
    '- entrypoint-responsibility-creep: handler/main/cli piling parse + persist + cache + join in one fn',
    '- duplicate-contract-ownership: second copy of API version / artifact role / status vocabulary',
    '- rough-temp-persistence: staging/tmp paths in durable dirs without cleanup boundary',
    '- docs-cleaner-than-code: rich NOTICE/handoff while structure still duplicates or fattens',
    '',
    'Signals are heuristic. Fix real issues or note false positives in one line.',
    '',
  ];

  for (const entry of queue.entries.slice(-4)) {
    lines.push(`File: ${entry.filePath}`);
    for (const finding of entry.findings || []) {
      lines.push(`  - [${finding.severity}] ${finding.code}`);
      lines.push(`    evidence: ${finding.evidence}`);
      lines.push(`    why: ${finding.why_it_matters}`);
      lines.push(`    action: ${finding.suggested_action}`);
    }
    lines.push('');
  }

  let context = lines.join('\n').trim();
  if (context.length > maxChars) {
    context = `${context.slice(0, maxChars - 40).trimEnd()}\n...[code-quality queue truncated]`;
  }

  return { context, queue, hadPending: true };
}

function clearQueue() {
  try {
    fs.unlinkSync(queuePath());
  } catch {
    /* best-effort */
  }
}

function summarizeForStop(queue = readQueue()) {
  const findings = queue.entries.flatMap(entry =>
    (entry.findings || []).map(finding => ({ ...finding, filePath: entry.filePath })),
  );
  const high = findings.filter(item => item.severity === 'high');
  const paths = [...new Set(queue.allPaths)];
  return { findings, high, paths, entryCount: queue.entries.length };
}

module.exports = {
  upsertReviewEntry,
  consumePendingContext,
  clearQueue,
  readQueue,
  summarizeForStop,
  reviewFingerprint,
  findingFingerprint,
};
