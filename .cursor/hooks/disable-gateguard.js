#!/usr/bin/env node
'use strict';

/**
 * Cursor sessionStart / subagentStart hook — disable ECC GateGuard for this session.
 *
 * Cursor passes session-scoped `env` from hook output to later hooks, including
 * ECC `gateguard-fact-force.js` (PreToolUse Edit/Write/Bash).
 */

const GATEGUARD_HOOK_IDS =
  'pre:edit-write:gateguard-fact-force,pre:bash:gateguard-fact-force';

function buildDisableGateGuardPayload(extraContextLines = []) {
  const context = [
    'GateGuard is disabled for this AUV workspace (ECC_GATEGUARD=off).',
    `ECC_DISABLED_HOOKS=${GATEGUARD_HOOK_IDS}`,
    ...extraContextLines.filter(line => typeof line === 'string' && line.trim()),
  ];

  return {
    env: {
      ECC_GATEGUARD: 'off',
      GATEGUARD_DISABLED: '1',
      ECC_DISABLED_HOOKS: GATEGUARD_HOOK_IDS,
    },
    additional_context: context.join('\n'),
  };
}

function main() {
  process.stdout.write(`${JSON.stringify(buildDisableGateGuardPayload())}\n`);
  process.exit(0);
}

module.exports = {
  GATEGUARD_HOOK_IDS,
  buildDisableGateGuardPayload,
};

if (require.main === module) {
  main();
}
