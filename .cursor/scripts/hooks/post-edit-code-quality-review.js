'use strict';

const crypto = require('crypto');
const path = require('path');
const { reviewEditedFile, readFileSafe, shouldReview } = require('../lib/code-quality-heuristics');
const { upsertReviewEntry, readQueue } = require('../lib/session-code-quality-queue');

const MAX_STDIN = 1024 * 1024;

function getFilePaths(input) {
  const cursorPath = input?.file_path || input?.path || input?.file;
  if (cursorPath) {
    return [String(cursorPath)];
  }

  const toolPath = input?.tool_input?.file_path;
  if (toolPath) {
    return [String(toolPath)];
  }

  const edits = input?.tool_input?.edits || input?.edits;
  if (Array.isArray(edits)) {
    return [...new Set(edits.map(edit => String(edit?.file_path || '')).filter(Boolean))];
  }

  return [];
}

function getEdits(input) {
  return input?.edits || input?.tool_input?.edits || [];
}

function contentFingerprint(filePath, edits) {
  const content = readFileSafe(filePath);
  const editBlob = JSON.stringify(edits || []);
  return crypto.createHash('sha1').update(content).update(editBlob).digest('hex').slice(0, 12);
}

function formatFindingLine(finding) {
  return `  [${finding.severity}] ${finding.code} in \`${finding.file}\`\n    evidence: ${finding.evidence}\n    why: ${finding.why_it_matters}\n    action: ${finding.suggested_action}`;
}

function run(rawInput, hookMeta = {}) {
  let input = {};
  try {
    input = typeof rawInput === 'string' ? JSON.parse(rawInput || '{}') : rawInput || {};
  } catch {
    return { exitCode: 0, stdout: typeof rawInput === 'string' ? rawInput : '' };
  }

  const rawOut = typeof rawInput === 'string' ? rawInput : JSON.stringify(input);
  const filePaths = getFilePaths(input).filter(p => shouldReview(p));
  if (filePaths.length === 0) {
    return { exitCode: 0, stdout: rawOut };
  }

  const edits = getEdits(input);
  const sessionPaths = readQueue().allPaths;
  const stderrLines = [];

  for (const filePath of filePaths) {
    const review = reviewEditedFile({
      filePath,
      edits,
      sessionPaths,
    });
    upsertReviewEntry(review, {
      source: hookMeta.source || '',
      toolUseId: hookMeta.toolUseId || input.tool_use_id || '',
      contentHash: contentFingerprint(filePath, edits),
    });

    if (review.findings.length > 0) {
      stderrLines.push(`[AUV code-quality] ${review.filePath}`);
      for (const finding of review.findings) {
        stderrLines.push(formatFindingLine(finding));
      }
    }
  }

  if (stderrLines.length > 0) {
    stderrLines.push('[AUV code-quality] Re-check structure before the next edit — signals are heuristic, not correctness.');
  }

  return {
    exitCode: 0,
    stdout: rawOut,
    stderr: stderrLines.join('\n'),
  };
}

if (require.main === module) {
  let raw = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', chunk => {
    if (raw.length < MAX_STDIN) {
      raw += chunk.substring(0, MAX_STDIN - raw.length);
    }
  });
  process.stdin.on('end', () => {
    const result = run(raw);
    if (result.stderr) {
      process.stderr.write(`${result.stderr}\n`);
    }
    process.stdout.write(result.stdout);
    process.exit(result.exitCode || 0);
  });
}

module.exports = { run };
