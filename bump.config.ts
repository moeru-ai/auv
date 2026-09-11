import { readFile, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import { cwd } from 'node:process'

import { parse, patch } from '@decimalturn/toml-patch'
import { defineConfig } from 'bumpp'
import { x } from 'tinyexec'

async function syncCargoToml() {
  const cargoTomlPath = join(cwd(), 'Cargo.toml')
  const cargoTomlSource = await readFile(cargoTomlPath, 'utf8')
  const cargoToml = parse(cargoTomlSource) as {
    workspace?: {
      package?: {
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

  // NOTICE: here we don't use import package.json because during bumpp, the package.json will be updated,
  // and the import will be cached, yet the Cargo.toml will be updated with the old version.
  const packageJSONFile = join(cwd(), 'package.json')
  const packageJSON = JSON.parse(await readFile(packageJSONFile, 'utf-8'))
  if (typeof packageJSON?.version !== 'string' || packageJSON?.version === null) {
    throw new TypeError('package.json does not contain a valid version')
  }

  cargoToml.workspace.package.version = packageJSON.version
  console.info(`Bumping Cargo.toml version to ${cargoToml.workspace.package.version} (from package.json, ${packageJSON.version})`)

  await writeFile(
    cargoTomlPath,
    patch(cargoTomlSource, cargoToml),
  )
}

export default defineConfig({
  all: true,
  commit: 'release: v%s',
  execute: async () => {
    await x('pnpm', ['publish', '-r', '--access', 'public', '--no-git-checks', '--dry-run'])

    await syncCargoToml()
    await x('cargo', ['generate-lockfile'])
  },
  push: false,
  recursive: true,
  sign: false,
})
