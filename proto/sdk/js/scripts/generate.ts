import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { findWorkspaceDir } from '@pnpm/find-workspace-dir'
import { x } from 'tinyexec'

const packageDirectory = join(dirname(fileURLToPath(import.meta.url)), '..')
const root = await findWorkspaceDir(packageDirectory)
if (root === undefined)
  throw new Error('@auv-js/api-client generation is not inside a pnpm workspace')

const protoDirectory = join(root, 'proto')
await x('openapi-ts', [
  '-i',
  join(protoDirectory, 'openapi', 'auv-daemon.swagger.json'),
  '-o',
  join(packageDirectory, 'src', 'gen'),
], {
  nodeOptions: { cwd: packageDirectory },
  throwOnError: true,
})
