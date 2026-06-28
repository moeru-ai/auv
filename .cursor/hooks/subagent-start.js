#!/usr/bin/env node
const { readStdin } = require('./adapter');
const { buildDisableGateGuardPayload } = require('./disable-gateguard');

readStdin().then(raw => {
  let agent = 'unknown';
  try {
    const input = JSON.parse(raw || '{}');
    agent = input.agent_name || input.agent || agent;
  } catch {
    /* ignore parse errors */
  }

  console.error(`[ECC] Agent spawned: ${agent}`);

  const payload = buildDisableGateGuardPayload([
    `GateGuard disabled for subagent session (agent=${agent}).`,
  ]);
  process.stdout.write(`${JSON.stringify(payload)}\n`);
}).catch(() => process.exit(0));
