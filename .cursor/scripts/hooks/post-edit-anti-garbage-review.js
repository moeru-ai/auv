'use strict';

const path = require('path');
const { reviewEditedFile } = require('../lib/anti-garbage-heuristics');
const {
  appendReviewEntry,
  readQueue,
} = require('../lib/session-edit-review-queue');

const MAX_STDIN = 1024 * 1024;
const SKIP_PATH_RE = /(?:^|\/)(?:node_modules|target|\.git|dist|build)\//;

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

function shouldReview(filePath) {
  const rel = path.relative(process.cwd(), path.resolve(filePath)).replace(/\\/g, '/');
  if (!rel || rel.startsWith('..')) {
    return false;
  }
  if (SKIP_PATH_RE.test(rel)) {
    return false;
  }
  return /\.(rs|toml|js|jsx|ts|tsx|md|proto)$/.test(rel) || rel.startsWith('src/') || rel.startsWith('crates/');
}

function run(rawInput) {
  let input = {};
  try {
    input = typeof rawInput === 'string' ? JSON.parse(rawInput || '{}') : rawInput || {};
  } catch {
    return { exitCode: 0, stdout: typeof rawInput === 'string' ? rawInput : '' };
  }

  const rawOut = typeof rawInput === 'string' ? rawInput : JSON.stringify(input);
  const filePaths = getFilePaths(input).filter(shouldReview);
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
    appendReviewEntry(review);

    if (review.findings.length > 0) {
      stderrLines.push(`[AUV anti-garbage] ${filePath}`);
      for (const finding of review.findings) {
        stderrLines.push(`  [${finding.severity}] ${finding.code}: ${finding.message}`);
      }
    }
  }

  if (stderrLines.length > 0) {
    stderrLines.push('[AUV anti-garbage] Re-check before the next edit: simpler inline code, narrower slice, hermetic tests.');
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
