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
  return path.join(os.tmpdir(), `ecc-anti-garbage-queue-${sessionKey()}.json`);
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

function appendReviewEntry(entry) {
  const queue = readQueue();
  queue.entries.push({
    ...entry,
    timestamp: new Date().toISOString(),
  });
  if (entry.filePath) {
    queue.allPaths.push(entry.filePath);
  }
  writeQueue(queue);
  return queue;
}

function consumePendingContext(maxChars = 2800) {
  const queue = readQueue();
  if (queue.entries.length === 0) {
    return { context: '', queue, hadPending: false };
  }

  const lines = [
    '[AUV anti-garbage edit review]',
    'Before writing more code, re-check the latest edit(s) against these failure modes:',
    '- fake abstraction: new helper/trait/wrapper that only forwards',
    '- cross-layer mix: runtime + CLI + proto + read-side + docs in one slice',
    '- fake refactor: rename/move noise hiding behavior changes',
    '- env-coupled tests: desktop/window/network state without live/integration label',
    '- premature extraction: donor-specific logic flattened into generic helper',
    '',
    'If a signal is real, simplify inline or split the slice before continuing.',
    '',
  ];

  for (const entry of queue.entries.slice(-4)) {
    lines.push(`File: ${entry.filePath}`);
    if (entry.layers?.length) {
      lines.push(`  layers touched this session: ${entry.layers.join(', ')}`);
    }
    for (const finding of entry.findings || []) {
      lines.push(`  - [${finding.code}] ${finding.message}`);
    }
    lines.push('');
  }

  let context = lines.join('\n').trim();
  if (context.length > maxChars) {
    context = `${context.slice(0, maxChars - 40).trimEnd()}\n...[review queue truncated]`;
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
  const findings = queue.entries.flatMap(entry => entry.findings || []);
  const high = findings.filter(item => item.severity === 'high');
  const paths = [...new Set(queue.allPaths)];
  return { findings, high, paths, entryCount: queue.entries.length };
}

module.exports = {
  appendReviewEntry,
  consumePendingContext,
  clearQueue,
  readQueue,
  summarizeForStop,
};
