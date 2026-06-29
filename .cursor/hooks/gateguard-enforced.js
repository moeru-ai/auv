#!/usr/bin/env node
/**
 * GateGuard hard-lock marker for this workspace.
 *
 * When this file exists, Fact-Forcing Gate cannot be disabled via:
 * - ECC_GATEGUARD=off (or any ECC_GATEGUARD disable value)
 * - GATEGUARD_DISABLED=1
 * - ECC_DISABLED_HOOKS including gateguard hook ids
 *
 * Recovery path: present required facts in chat, then retry the same tool call.
 *
 * To opt out project-wide (not recommended), delete this file and add
 * `.cursor/hooks/disable-gateguard.js` instead.
 */

'use strict';

module.exports = {
  policy: 'gateguard-enforced',
};
