import type { DeviceCredential } from '../transport/connection'

import process from 'node:process'

import { Buffer } from 'node:buffer'
import { readSync } from 'node:fs'

/** Set by a parent that sends this process a credential on its stdin. */
export const CREDENTIAL_STDIN_ENV = 'AUV_CREDENTIAL_STDIN'

let inherited: undefined | { value: DeviceCredential | undefined }

/**
 * Returns the credential a parent sent on stdin, read once per process, or
 * undefined when none was sent. The daemon uses this channel for Runners
 * because other processes of the same user can read the environment. Call it
 * before anything else reads stdin, then pass the result to `connect` or
 * `connectFromContext` as `credential`.
 */
export function inheritedCredential(env: NodeJS.ProcessEnv = process.env): DeviceCredential | undefined {
  if (env[CREDENTIAL_STDIN_ENV] === undefined)
    return undefined
  if (inherited !== undefined)
    return inherited.value

  // One byte at a time, so input after the first line stays unread.
  const bytes: number[] = []
  const byte = Buffer.alloc(1)
  while (readSync(0, byte, 0, 1, null) === 1 && byte[0] !== 0x0A)
    bytes.push(byte[0]!)
  const value = Buffer.from(bytes).toString('utf8').replace(/\r$/, '')
  inherited = { value: value === '' ? undefined : value }
  return inherited.value
}
