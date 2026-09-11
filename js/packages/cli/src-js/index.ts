import { readFileSync } from 'node:fs'

import { nativePackageVersion } from '../binding.js'

const packageVersion = (JSON.parse(
  readFileSync(new URL('../package.json', import.meta.url), 'utf8'),
) as { version: string }).version

const bindingVersion = nativePackageVersion()
if (bindingVersion !== packageVersion) {
  console.warn(`The AUV native binding version (${bindingVersion}) does not match @auv-js/cli (${packageVersion}). This may cause unexpected behavior.`)
}

export * from '../binding.js'
export * from './binary.js'
