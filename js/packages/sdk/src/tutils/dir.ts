import { dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

import { findWorkspaceDir } from '@pnpm/find-workspace-dir'

export async function repositoryRoot(): Promise<string> {
  const root = await findWorkspaceDir(dirname(fileURLToPath(import.meta.url)))
  if (root === undefined)
    throw new Error('AUV test utilities are not inside a pnpm workspace')

  return root
}
