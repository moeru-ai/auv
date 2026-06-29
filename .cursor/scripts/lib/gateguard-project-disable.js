#!/usr/bin/env node
/**
 * Project-local GateGuard policy markers.
 *
 * - `.cursor/hooks/gateguard-enforced.js` — GateGuard cannot be disabled via
 *   env (`ECC_GATEGUARD=off`, `GATEGUARD_DISABLED`) or `ECC_DISABLED_HOOKS`.
 * - `.cursor/hooks/disable-gateguard.js` — opt-out marker (ignored when enforced).
 */

'use strict';

const fs = require('fs');
const path = require('path');

const DISABLE_MARKER_SEGMENTS = ['.cursor', 'hooks', 'disable-gateguard.js'];
const ENFORCED_MARKER_SEGMENTS = ['.cursor', 'hooks', 'gateguard-enforced.js'];

function enforcedMarkerPath(projectRoot) {
  return path.join(projectRoot, ...ENFORCED_MARKER_SEGMENTS);
}

function hasEnforcedMarkerAt(projectRoot) {
  const marker = enforcedMarkerPath(projectRoot);
  try {
    return fs.existsSync(marker) && fs.statSync(marker).isFile();
  } catch (_) {
    return false;
  }
}

function disableMarkerPath(projectRoot) {
  return path.join(projectRoot, ...DISABLE_MARKER_SEGMENTS);
}

function hasDisableMarkerAt(projectRoot) {
  const marker = disableMarkerPath(projectRoot);
  try {
    return fs.existsSync(marker) && fs.statSync(marker).isFile();
  } catch (_) {
    return false;
  }
}

function collectSearchRoots(extraStarts = []) {
  const roots = [];
  const push = value => {
    const raw = String(value || '').trim();
    if (!raw) {
      return;
    }
    roots.push(raw);
  };

  push(process.cwd());
  for (const key of ['CLAUDE_PROJECT_DIR', 'CURSOR_WORKSPACE', 'WORKSPACE_FOLDER']) {
    push(process.env[key]);
  }

  for (const value of extraStarts) {
    push(value);
  }

  return roots;
}

function isGateGuardEnforced(extraStarts = []) {
  const seen = new Set();

  for (const start of collectSearchRoots(extraStarts)) {
    let dir = path.resolve(start);
    if (seen.has(dir)) {
      continue;
    }
    seen.add(dir);

    try {
      if (fs.existsSync(dir) && fs.statSync(dir).isFile()) {
        dir = path.dirname(dir);
      }
    } catch (_) {
      /* ignore */
    }

    while (dir && dir !== path.dirname(dir)) {
      if (hasEnforcedMarkerAt(dir)) {
        return true;
      }
      dir = path.dirname(dir);
    }
  }

  return false;
}

function isProjectGateGuardDisabled(extraStarts = []) {
  if (isGateGuardEnforced(extraStarts)) {
    return false;
  }

  const seen = new Set();

  for (const start of collectSearchRoots(extraStarts)) {
    let dir = path.resolve(start);
    if (seen.has(dir)) {
      continue;
    }
    seen.add(dir);

    try {
      if (fs.existsSync(dir) && fs.statSync(dir).isFile()) {
        dir = path.dirname(dir);
      }
    } catch (_) {
      /* ignore */
    }

    while (dir && dir !== path.dirname(dir)) {
      if (hasDisableMarkerAt(dir)) {
        return true;
      }
      dir = path.dirname(dir);
    }
  }

  return false;
}

module.exports = {
  DISABLE_MARKER_SEGMENTS,
  ENFORCED_MARKER_SEGMENTS,
  disableMarkerPath,
  enforcedMarkerPath,
  hasDisableMarkerAt,
  hasEnforcedMarkerAt,
  isGateGuardEnforced,
  isProjectGateGuardDisabled,
};
