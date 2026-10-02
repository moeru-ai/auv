import { readFile, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import { cwd } from 'node:process'

import { parse, patch } from '@decimalturn/toml-patch'
import { defineConfig } from 'bumpp'
import { x } from 'tinyexec'

interface CargoDependency {
  path?: string
  version?: string
}

interface CargoDependencyTables {
  'build-dependencies'?: Record<string, CargoDependency | string>
  'dependencies'?: Record<string, CargoDependency | string>
  'dev-dependencies'?: Record<string, CargoDependency | string>
}

interface CargoManifest extends CargoDependencyTables {
  package?: {
    publish?: boolean | string[] | { workspace: boolean }
    version?: string
  }
  target?: Record<string, CargoDependencyTables>
}

async function syncCargoToml() {
  const cargoTomlPath = join(cwd(), 'Cargo.toml')
  const cargoTomlSource = await readFile(cargoTomlPath, 'utf8')
  const cargoToml = parse(cargoTomlSource) as {
    workspace?: {
      members?: string[]
      package?: {
        publish?: boolean | string[]
        version?: string
      }
    }
  }

  if (typeof cargoToml !== 'object' || cargoToml === null) {
    throw new TypeError('Cargo.toml does not contain a valid object')
  }
  if (typeof cargoToml.workspace?.package?.version !== 'string') {
    throw new TypeError('Cargo.toml does not contain a valid version in workspace.package.version')
  }
  if (!Array.isArray(cargoToml.workspace.members)) {
    throw new TypeError('Cargo.toml does not contain workspace members')
  }

  // NOTICE: here we don't use import package.json because during bumpp, the package.json will be updated,
  // and the import will be cached, yet the Cargo.toml will be updated with the old version.
  const packageJSONFile = join(cwd(), 'package.json')
  const packageJSON = JSON.parse(await readFile(packageJSONFile, 'utf-8'))
  if (typeof packageJSON?.version !== 'string' || packageJSON?.version === null) {
    throw new TypeError('package.json does not contain a valid version')
  }

  const oldVersion = cargoToml.workspace.package.version
  const newVersion = packageJSON.version
  const memberUpdates: Array<{ path: string, source: string }> = []

  for (const member of cargoToml.workspace.members) {
    const memberPath = join(cwd(), member, 'Cargo.toml')
    const memberSource = await readFile(memberPath, 'utf8')
    const memberToml = parse(memberSource) as CargoManifest
    const publish = memberToml.package?.publish
    if (publish === false || (typeof publish === 'object' && !Array.isArray(publish) && publish.workspace && cargoToml.workspace.package.publish === false)) {
      continue
    }

    // Cargo strips local paths during publishing, so each path dependency
    // needs a registry version that follows the workspace's release version.
    let changed = false
    for (const table of [memberToml, ...Object.values(memberToml.target ?? {})]) {
      for (const section of ['dependencies', 'build-dependencies', 'dev-dependencies'] as const) {
        for (const [name, dependency] of Object.entries(table[section] ?? {})) {
          if (typeof dependency === 'string' || typeof dependency?.path !== 'string') {
            continue
          }
          if (dependency.version !== oldVersion) {
            throw new Error(`${member}: ${name} must require the current workspace version ${oldVersion}`)
          }
          dependency.version = newVersion
          changed = true
        }
      }
    }
    if (changed) {
      memberUpdates.push({ path: memberPath, source: patch(memberSource, memberToml) })
    }
  }

  // The N-API crate is an independent, unpublished Cargo workspace, so the
  // member loop above cannot discover it. Its compiled version must still
  // match the npm package version checked by the JavaScript entrypoint.
  const nativeManifestPath = join(cwd(), 'js/packages/cli/Cargo.toml')
  const nativeManifestSource = await readFile(nativeManifestPath, 'utf8')
  const nativeManifest = parse(nativeManifestSource) as CargoManifest
  if (nativeManifest.package?.version !== oldVersion) {
    throw new Error(`js/packages/cli/Cargo.toml must use the current workspace version ${oldVersion}`)
  }
  nativeManifest.package.version = newVersion
  memberUpdates.push({ path: nativeManifestPath, source: patch(nativeManifestSource, nativeManifest) })

  cargoToml.workspace.package.version = newVersion
  console.info(`Bumping Cargo.toml and ${memberUpdates.length} member manifests to ${newVersion}`)

  await writeFile(
    cargoTomlPath,
    patch(cargoTomlSource, cargoToml),
  )
  for (const update of memberUpdates) {
    await writeFile(update.path, update.source)
  }
}

export default defineConfig({
  all: true,
  commit: 'release: v%s',
  execute: async () => {
    await x('pnpm', ['publish', '-r', '--access', 'public', '--no-git-checks', '--dry-run'])

    await syncCargoToml()
    await x('cargo', ['generate-lockfile'])
    // The N-API crate is a separate Cargo workspace with its own committed
    // lockfile, and release builds pass `--locked`. Refresh only the versions
    // of its local path packages; leave registry dependencies untouched.
    await x('cargo', ['update', '--workspace', '--offline', '--manifest-path', 'js/packages/cli/Cargo.toml'])
  },
  push: false,
  recursive: true,
  sign: false,
})
