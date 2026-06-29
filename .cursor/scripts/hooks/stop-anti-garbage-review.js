'use strict';

const { summarizeForStop, clearQueue } = require('../lib/session-edit-review-queue');

function buildFollowup({ paths, high, loopCount }) {
  if (loopCount >= 1 || high.length === 0) {
    return null;
  }

  const relPaths = paths
    .map(p => p.replace(/\\/g, '/'))
    .filter(p => p.includes('src/') || p.includes('crates/') || p.includes('.cursor/'))
    .slice(-8);

  if (relPaths.length === 0) {
    return null;
  }

  const focus = [
    'fake abstraction / pass-through wrapper',
    'cross-layer mix',
    'fake refactor hiding behavior',
    'env-coupled tests',
    'premature extraction',
  ].join('; ');

  return [
    'Anti-garbage review required before ending this turn.',
    `Launch 2 parallel code-reviewer subagents (Composer 2.5) on the session diff for: ${relPaths.join(', ')}.`,
    `Hunt for: ${focus}.`,
    'If findings are valid, simplify or split the slice now. If false positive, note why in one line each.',
  ].join(' ');
}

function run(rawInput) {
  let input = {};
  try {
    input = typeof rawInput === 'string' ? JSON.parse(rawInput || '{}') : rawInput || {};
  } catch {
    input = {};
  }

  const loopCount = Number(input.loop_count) || 0;
  const summary = summarizeForStop();
  const stderr = [];

  if (summary.findings.length > 0) {
    stderr.push(`[AUV anti-garbage] stop summary: ${summary.findings.length} signal(s), ${summary.high.length} high.`);
    for (const finding of summary.high.slice(0, 6)) {
      stderr.push(`  - ${finding.code}: ${finding.message}`);
    }
  }

  const followup_message = buildFollowup({
    paths: summary.paths,
    high: summary.high,
    loopCount,
  });

  if (followup_message && loopCount < 1) {
    clearQueue();
  }

  return {
    stdout: JSON.stringify(followup_message ? { followup_message } : {}),
    stderr: stderr.join('\n'),
    exitCode: 0,
  };
}

if (require.main === module) {
  let raw = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', chunk => { raw += chunk; });
  process.stdin.on('end', () => {
    const result = run(raw);
    if (result.stderr) {
      process.stderr.write(`${result.stderr}\n`);
    }
    process.stdout.write(result.stdout || '{}');
    process.exit(0);
  });
}

module.exports = { run, buildFollowup };
