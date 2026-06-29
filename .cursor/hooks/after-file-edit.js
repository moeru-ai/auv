#!/usr/bin/env node
const { hookEnabled, readStdin, runExistingHook, transformToClaude } = require('./adapter');
const { run: runAntiGarbageReview } = require('../scripts/hooks/post-edit-anti-garbage-review');
const { run: runCodeQualityReview } = require('../scripts/hooks/post-edit-code-quality-review');

readStdin().then(raw => {
  try {
    const input = JSON.parse(raw);
    const claudeInput = transformToClaude(input, {
      tool_input: {
        file_path: input.path || input.file || input.file_path || '',
        edits: input.edits || [],
      },
    });
    const claudeStr = JSON.stringify(claudeInput);

    runExistingHook('post-edit-accumulator.js', claudeStr);
    runExistingHook('post-edit-console-warn.js', claudeStr);
    if (hookEnabled('post:edit:design-quality-check', ['standard', 'strict'])) {
      runExistingHook('design-quality-check.js', claudeStr);
    }

    const review = runAntiGarbageReview(claudeStr, { source: 'afterFileEdit' });
    if (review.stderr) {
      process.stderr.write(`${review.stderr}\n`);
    }

    const quality = runCodeQualityReview(claudeStr, { source: 'afterFileEdit' });
    if (quality.stderr) {
      process.stderr.write(`${quality.stderr}\n`);
    }
  } catch {}
  process.stdout.write(raw);
}).catch(() => process.exit(0));
