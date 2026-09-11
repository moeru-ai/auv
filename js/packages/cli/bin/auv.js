#!/usr/bin/env node

import process from 'node:process'

import { spawnSync } from 'node:child_process'
import { constants } from 'node:os'

import { binaryPath } from '../dist/binary.js'

try {
  const result = spawnSync(binaryPath(), process.argv.slice(2), {
    stdio: 'inherit',
    windowsHide: true,
  })

  if (result.error) {
    throw result.error
  }

  if (result.signal) {
    try {
      process.kill(process.pid, result.signal)
    }
    catch {
      const signalNumber = constants.signals[result.signal]
      process.exitCode = signalNumber === undefined ? 1 : 128 + signalNumber
    }
  }
  else {
    process.exitCode = result.status ?? 1
  }
}
catch (error) {
  const message = error && typeof error === 'object' && 'message' in error
    ? String(error.message)
    : String(error)
  console.error(`auv: ${message}`)
  process.exitCode = 1
}
