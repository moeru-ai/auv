import type { Buffer } from 'node:buffer'

import process from 'node:process'

import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { fileURLToPath } from 'node:url'

import { describe, expect, it } from 'vitest'

async function readAsChild(stdin: string, env: NodeJS.ProcessEnv): Promise<string> {
  const child = spawn(process.execPath, ['--import', 'tsx', fileURLToPath(new URL('../tutils/inherited-credential.ts', import.meta.url))], {
    env: { ...process.env, ...env },
    stdio: ['pipe', 'pipe', 'inherit'],
  })
  child.stdin.end(stdin)
  let output = ''
  child.stdout.on('data', (chunk: Buffer) => output += chunk.toString())
  await once(child, 'exit')
  return output
}

describe('inheritedCredential', () => {
  it('reads only the first stdin line when the parent sets the flag', async () => {
    expect(JSON.parse(await readAsChild('secret-token\nlater input\n', { AUV_CREDENTIAL_STDIN: '1' })))
      .toEqual({ credential: 'secret-token', rest: 'later input\n' })
  })

  it('ignores stdin when the parent did not set the flag', async () => {
    expect(JSON.parse(await readAsChild('secret-token\n', { AUV_CREDENTIAL_STDIN: undefined })))
      .toEqual({ credential: null, rest: 'secret-token\n' })
  })
})
