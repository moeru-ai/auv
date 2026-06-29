#!/usr/bin/env node
'use strict';

const { readStdin } = require('./adapter');
const { run: runAntiGarbageReview } = require('../scripts/hooks/post-edit-anti-garbage-review');

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
    });

    const review = runAntiGarbageReview(claudeInput);
    if (review.stderr) {
      process.stderr.write(`${review.stderr}\n`);
    }

    const queueContext = review.pendingContext;
    if (queueContext) {
      process.stdout.write(`${JSON.stringify({ additional_context: queueContext })}\n`);
      return;
    }

    process.stdout.write(raw);
  })
  .catch(() => process.exit(0));
