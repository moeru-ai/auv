import { readFileSync } from 'node:fs'

// eslint-disable-next-line no-restricted-syntax -- the napi binding is a runtime file that tsdown never bundles by this exact specifier.
import { nativePackageVersion } from '../binding.js'

const packageVersion = (JSON.parse(
  readFileSync(new URL('../package.json', import.meta.url), 'utf8'),
) as { version: string }).version

const bindingVersion = nativePackageVersion()
if (bindingVersion !== packageVersion) {
  console.warn(`The AUV native binding version (${bindingVersion}) does not match @auv-js/cli (${packageVersion}). This may cause unexpected behavior.`)
}

// eslint-disable-next-line no-restricted-syntax -- the napi binding is a runtime file that tsdown never bundles by this exact specifier.
export { nativePackageVersion } from '../binding.js'
export * from './binary'
export * from './setup'
