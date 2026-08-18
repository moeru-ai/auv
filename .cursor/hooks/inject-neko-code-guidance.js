#!/usr/bin/env node
'use strict';

const { readStdin } = require('./adapter');
const {
  formatDocSection,
  readDoc,
  workspaceRootsFromInput,
} = require('../scripts/lib/read-project-context');

const GUIDANCE_DOC = {
  filename: 'docs/ai/references/ops/2026-08-03-neko-code-study-and-anti-spaghetti-guidance-note.md',
  envPath: 'AUV_NEKO_CODE_GUIDANCE_PATH',
  maxCharsEnv: 'AUV_NEKO_CODE_GUIDANCE_MAX_CHARS',
  defaultMaxChars: 5_000,
};

function hookEventName(input) {
  return String(
    input.hook_event_name || input.hookEventName || input._cursor?.hook_event_name || '',
  ).trim();
}

readStdin()
  .then(raw => {
    let input = {};
    try {
      input = JSON.parse(raw || '{}');
    } catch {
      input = {};
    }

    // Read the durable guidance exactly once for each beforeSubmitPrompt event.
    const section = formatDocSection(readDoc(GUIDANCE_DOC, workspaceRootsFromInput(input)));
    if (!section) {
      process.stdout.write(raw);
      return;
    }

    const payload = {
      additional_context: [
        '[AUV code-study guidance — read once for this prompt]',
        section,
      ].join('\n\n'),
    };

    if (hookEventName(input) === 'beforeSubmitPrompt') {
      payload.continue = true;
    }

    process.stdout.write(`${JSON.stringify(payload)}\n`);
  })
  .catch(() => process.exit(0));
