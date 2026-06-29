#!/usr/bin/env node
'use strict';

const { readStdin } = require('./adapter');
const { consumePendingContext } = require('../scripts/lib/session-code-quality-queue');

readStdin()
  .then(raw => {
    let input = {};
    try {
      input = JSON.parse(raw || '{}');
    } catch {
      input = {};
    }

    const { context, hadPending } = consumePendingContext();
    if (!hadPending || !context) {
      process.stdout.write(raw);
      return;
    }

    const payload = {
      continue: true,
      additional_context: context,
    };

    if (input.hook_event_name === 'preCompact') {
      payload.user_message = 'Compaction pending — re-read code-quality edit review queue.';
    }

    process.stdout.write(`${JSON.stringify(payload)}\n`);
  })
  .catch(() => process.exit(0));
