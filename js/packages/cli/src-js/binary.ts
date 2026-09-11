import process from 'node:process'

import { accessSync, constants } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join } from 'node:path'

const require = createRequire(import.meta.url)

const PLATFORM_PACKAGES = new Map<string, readonly [string, string]>([
  ['darwin-arm64', ['@auv-js/cli-darwin-arm64', 'bin/auv']],
  ['darwin-x64', ['@auv-js/cli-darwin-x64', 'bin/auv']],
  ['linux-arm64', ['@auv-js/cli-linux-arm64-gnu', 'bin/auv']],
  ['linux-x64', ['@auv-js/cli-linux-x64-gnu', 'bin/auv']],
  ['win32-x64', ['@auv-js/cli-win32-x64-msvc', 'bin/auv.exe']],
])

interface DiagnosticReport {
  header?: {
    glibcVersionRuntime?: string
  }
}

export class AuvBinaryError extends Error {
  constructor(message: string, options?: ErrorOptions) {
    super(message, options)
    this.name = 'AuvBinaryError'
  }
}

/** Return the absolute path of the AUV executable installed for this host. */
export function binaryPath(): string {
  const platformKey = `${process.platform}-${process.arch}`

  const platformPackage = PLATFORM_PACKAGES.get(platformKey)
  if (!platformPackage) {
    throw new AuvBinaryError(`AUV does not publish an executable for ${process.platform}/${process.arch}.`)
  }
  if (process.platform === 'linux' && isMusl()) {
    throw new AuvBinaryError('AUV currently publishes glibc Linux executables only; musl Linux is not supported.')
  }

  const [packageName, executable] = platformPackage
  let manifestPath: string

  try {
    manifestPath = require.resolve(`${packageName}/package.json`)
  }
  catch (cause) {
    throw new AuvBinaryError(`The optional package ${packageName} is missing. Reinstall @auv-js/cli without omitting optional dependencies.`, { cause })
  }

  const resolvedPath = join(dirname(manifestPath), executable)
  try {
    accessSync(resolvedPath, process.platform === 'win32' ? constants.F_OK : constants.X_OK)
  }
  catch (cause) {
    throw new AuvBinaryError(`The ${packageName} package does not contain an executable AUV binary at ${resolvedPath}.`, { cause })
  }

  return resolvedPath
}

function isMusl(): boolean {
  const report = process.report?.getReport?.() as DiagnosticReport | undefined
  return report?.header?.glibcVersionRuntime === undefined
}
