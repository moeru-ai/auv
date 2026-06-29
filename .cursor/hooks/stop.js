#!/usr/bin/env node
const { readStdin, runExistingHook, transformToClaude, hookEnabled } = require('./adapter');
const { run: runStopAntiGarbageReview } = require('../scripts/hooks/stop-anti-garbage-review');
const { run: runStopCodeQualityReview } = require('../scripts/hooks/stop-code-quality-review');

readStdin().then(raw => {
  const input = JSON.parse(raw || '{}');
  const claudeInput = transformToClaude(input);

  if (hookEnabled('stop:format-rust', ['standard', 'strict'])) {
    runExistingHook('stop-format-rust.js', claudeInput);
  }
  if (hookEnabled('stop:format-typecheck', ['standard', 'strict'])) {
    runExistingHook('stop-format-typecheck.js', claudeInput);
  }
  if (hookEnabled('stop:check-console-log', ['standard', 'strict'])) {
    runExistingHook('check-console-log.js', claudeInput);
  }
  if (hookEnabled('stop:session-end', ['minimal', 'standard', 'strict'])) {
    runExistingHook('session-end.js', claudeInput);
  }
  if (hookEnabled('stop:evaluate-session', ['minimal', 'standard', 'strict'])) {
    runExistingHook('evaluate-session.js', claudeInput);
  }
  if (hookEnabled('stop:cost-tracker', ['minimal', 'standard', 'strict'])) {
    runExistingHook('cost-tracker.js', claudeInput);
  }

  const antiGarbage = runStopAntiGarbageReview(raw);
  if (antiGarbage.stderr) {
    process.stderr.write(`${antiGarbage.stderr}\n`);
  }

  const codeQuality = runStopCodeQualityReview(raw);
  if (codeQuality.stderr) {
    process.stderr.write(`${codeQuality.stderr}\n`);
  }

  const antiGarbageFollowup = String(antiGarbage.stdout || '').trim();
  const codeQualityFollowup = String(codeQuality.stdout || '').trim();
  const followupPayload =
    antiGarbageFollowup && antiGarbageFollowup !== '{}'
      ? antiGarbageFollowup
      : codeQualityFollowup && codeQualityFollowup !== '{}'
        ? codeQualityFollowup
        : '';

  if (followupPayload) {
    process.stdout.write(`${followupPayload}\n`);
    return;
  }

  process.stdout.write(raw);
}).catch(() => process.exit(0));
