'use strict';

const { summarizeForStop, clearQueue } = require('../lib/session-code-quality-queue');

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
    'fixture duplication across tests',
    'fat handler/main/cli entrypoints',
    'duplicate contract constants',
    'rough staging/tmp persistence',
  ].join('; ');

  return [
    'Code-quality review required before ending this turn.',
    `Review session diff for: ${relPaths.join(', ')}.`,
    `Hunt for: ${focus}.`,
    'If valid, extract staging helper / split entrypoint / import contract owner. If false positive, note why in one line each.',
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
    stderr.push(`[AUV code-quality] stop summary: ${summary.findings.length} signal(s), ${summary.high.length} high.`);
    for (const finding of summary.high.slice(0, 6)) {
      stderr.push(`  - [high] ${finding.code} in ${finding.filePath || finding.file}: ${finding.evidence}`);
    }
  }

  const followup_message = buildFollowup({
    paths: summary.paths,
    high: summary.high,
    loopCount,
  });

  if (summary.entryCount > 0) {
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
