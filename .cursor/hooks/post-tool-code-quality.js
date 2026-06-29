#!/usr/bin/env node
'use strict';

const { readStdin } = require('./adapter');
const { run: runCodeQualityReview } = require('../scripts/hooks/post-edit-code-quality-review');

readStdin()
  .then(raw => {
    let input = {};
    try {
      input = JSON.parse(raw || '{}');
    } catch {
      process.stdout.write(raw);
      return;
    }

    const toolName = String(input.tool_name || '');
    if (!/^(Write|StrReplace)$/.test(toolName)) {
      process.stdout.write(raw);
      return;
    }

    const claudeInput = JSON.stringify({
      tool_input: {
        file_path: input.tool_input?.path || input.tool_input?.file_path || '',
        edits: input.tool_input?.edits || [],
      },
      tool_use_id: input.tool_use_id || '',
    });

    const review = runCodeQualityReview(claudeInput, {
      source: 'postToolUse',
      toolUseId: input.tool_use_id || '',
    });
    if (review.stderr) {
      process.stderr.write(`${review.stderr}\n`);
    }

    process.stdout.write(raw);
  })
  .catch(() => process.exit(0));
