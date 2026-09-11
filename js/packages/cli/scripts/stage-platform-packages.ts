import type { Buffer } from 'node:buffer'

import process from 'node:process'

import { execFileSync } from 'node:child_process'
import { chmod, copyFile, mkdir, readdir, readFile, rename, rm, writeFile } from 'node:fs/promises'
import { basename, dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import { findWorkspaceDir } from '@pnpm/find-workspace-dir'

const cliRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

interface PackageManifest {
  files?: string[]
  main: string
  name: string
  version: string
}

interface Target {
  archive: string
  executable: string
  packageDir: string
}

const targets: readonly Target[] = [
  {
    archive: 'auv-aarch64-apple-darwin.tar.gz',
    executable: 'auv',
    packageDir: 'darwin-arm64',
  },
  {
    archive: 'auv-x86_64-apple-darwin.tar.gz',
    executable: 'auv',
    packageDir: 'darwin-x64',
  },
  {
    archive: 'auv-aarch64-unknown-linux-gnu.tar.gz',
    executable: 'auv',
    packageDir: 'linux-arm64-gnu',
  },
  {
    archive: 'auv-x86_64-unknown-linux-gnu.tar.gz',
    executable: 'auv',
    packageDir: 'linux-x64-gnu',
  },
  {
    archive: 'auv-x86_64-pc-windows-msvc.zip',
    executable: 'auv.exe',
    packageDir: 'win32-x64-msvc',
  },
]

const artifactsRoot = resolve(process.argv[2] ?? join(cliRoot, 'artifacts'))
const rootManifest = JSON.parse(await readFile(join(cliRoot, 'package.json'), 'utf8')) as PackageManifest

const artifactFiles = await collectFiles(artifactsRoot)
for (const target of targets) {
  await stageTarget(target, artifactFiles, rootManifest.version)
}

await copyFile(join(await findWorkspaceDir(cliRoot) ?? cliRoot, 'LICENSE'), join(cliRoot, 'LICENSE'))
console.info(`Staged ${targets.length} AUV executables from ${artifactsRoot}`)

async function collectFiles(root: string): Promise<string[]> {
  const entries = await readdir(root, { withFileTypes: true })
  const nested = await Promise.all(
    entries.map(async (entry) => {
      const path = join(root, entry.name)
      return entry.isDirectory() ? collectFiles(path) : [path]
    }),
  )

  return nested.flat()
}

function extractExecutable(archive: string, executable: string): Buffer {
  const options = { encoding: 'buffer' as const, maxBuffer: 512 * 1024 * 1024 }
  if (archive.endsWith('.zip')) {
    return execFileSync('unzip', ['-p', archive, executable], options)
  }

  return execFileSync('tar', ['-xOf', archive, executable], options)
}

function preferNotarized(candidates: readonly string[]): string | undefined {
  return candidates.toSorted((left, right) => {
    const leftNotarized = left.includes('notarized-') ? 1 : 0
    const rightNotarized = right.includes('notarized-') ? 1 : 0
    return rightNotarized - leftNotarized
  })[0]
}

async function stageTarget(
  target: Target,
  artifactFiles: readonly string[],
  packageVersion: string,
): Promise<void> {
  const candidates = artifactFiles.filter(file => basename(file) === target.archive)
  const archive = preferNotarized(candidates)
  if (!archive) {
    throw new Error(`Missing release archive ${target.archive}`)
  }

  const packageRoot = join(cliRoot, 'npm', target.packageDir)
  const manifestPath = join(packageRoot, 'package.json')
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8')) as PackageManifest
  if (manifest.version !== packageVersion) {
    throw new Error(`${manifest.name} is ${manifest.version}, but @auv-js/cli is ${packageVersion}`)
  }

  const relativeExecutable = `bin/${target.executable}`
  if (!manifest.files?.includes(relativeExecutable)) {
    manifest.files = [...new Set([...(manifest.files ?? []), relativeExecutable])]
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
  }

  const executable = extractExecutable(archive, target.executable)
  const destination = join(packageRoot, relativeExecutable)
  const temporary = `${destination}.tmp`
  await mkdir(dirname(destination), { recursive: true })

  try {
    await writeFile(temporary, executable, { mode: 0o755 })
    await chmod(temporary, 0o755)
    await rename(temporary, destination)
  }
  finally {
    await rm(temporary, { force: true })
  }

  await copyFile(join(await findWorkspaceDir(cliRoot) ?? cliRoot, 'LICENSE'), join(packageRoot, 'LICENSE'))
}
