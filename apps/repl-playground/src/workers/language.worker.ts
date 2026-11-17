/// <reference lib="webworker" />
import type { CompilerOptions } from 'typescript'

import type { CompileRequest, CompileResult, LanguageWorkerApi } from '../runtime/protocol'

import ts from 'typescript'

import { createSystem, createVirtualTypeScriptEnvironment, knownLibFilesForCompilerOptions } from '@typescript/vfs'
import { createBirpc } from 'birpc'

// oxlint-disable-next-line import/default -- Vite `?raw` imports expose the file text as the default export.
import scriptApi from '../script-api/api.ts?raw'
import scriptEnv from '../script-api/script-env.d.txt?raw'

import { CellSyntaxError, compileCell } from '../stepper/compile'

// NOTICE(ts-lib-chunks): only ES libs are globbed; each `lib.*.d.ts` becomes a
// lazy raw chunk so nothing is fetched from a CDN. DOM libs are excluded on
// purpose: cells run in a Worker and should not see `document`.
const libLoaders = import.meta.glob('/node_modules/typescript/lib/lib.{es,decorators}*.d.ts', {
  import: 'default',
  query: '?raw',
}) as Record<string, () => Promise<string>>

const CELL = '/cell.ts'
const compilerOptions: CompilerOptions = {
  lib: ['es2023'],
  module: ts.ModuleKind.ESNext,
  // Cells use top-level await, which needs module detection.
  moduleDetection: ts.ModuleDetectionKind.Force,
  noEmit: true,
  strict: true,
  target: ts.ScriptTarget.ES2023,
}

let env: ReturnType<typeof createVirtualTypeScriptEnvironment> | undefined
let ready: Promise<void> | undefined
let current = ''

function compile(request: CompileRequest): CompileResult {
  try {
    const compiled = compileCell(request.source, { cellId: request.cellId, startLine: request.startLine })
    return { ok: true, ...compiled }
  }
  catch (error) {
    if (error instanceof CellSyntaxError) {
      const syntax = error as CellSyntaxError
      return { column: syntax.column, line: syntax.line, message: syntax.message, ok: false }
    }
    return { message: (error as Error).message, ok: false }
  }
}

async function init(source: string): Promise<void> {
  ready ??= (async () => {
    const files = new Map<string, string>()
    for (const lib of knownLibFilesForCompilerOptions(compilerOptions, ts)) {
      const load = libLoaders[`/node_modules/typescript/lib/${lib}`]
      if (load)
        files.set(`/${lib}`, await load())
    }
    files.set('/auv-api.ts', scriptApi)
    files.set('/script-env.d.ts', scriptEnv)
    current = source
    files.set(CELL, source || ' ')
    env = createVirtualTypeScriptEnvironment(createSystem(files), [CELL, '/auv-api.ts', '/script-env.d.ts'], ts, compilerOptions)
  })()
  await ready
}

/** Updates the cell file only when the text actually changed. */
function sync(source?: string): void {
  if (source === undefined || source === current)
    return
  current = source
  env?.updateFile(CELL, source || ' ')
}

const api: LanguageWorkerApi = {
  compile,
  completionDetail(pos, name, source) {
    sync(source)
    const detail = env?.languageService.getCompletionEntryDetails(CELL, pos, name, undefined, undefined, undefined, undefined)
    if (!detail)
      return null
    return {
      doc: ts.displayPartsToString(detail.documentation),
      type: ts.displayPartsToString(detail.displayParts),
    }
  },
  completions(pos, source) {
    sync(source)
    const result = env?.languageService.getCompletionsAtPosition(CELL, pos, {
      includeCompletionsWithInsertText: true,
    })
    if (!result)
      return { isMember: false, items: [] }
    return {
      isMember: result.isMemberCompletion,
      items: result.entries.map(entry => ({ kind: entry.kind, label: entry.name, sortText: entry.sortText })),
    }
  },
  diagnostics() {
    if (!env)
      return []
    const service = env.languageService
    return [...service.getSyntacticDiagnostics(CELL), ...service.getSemanticDiagnostics(CELL)].map(diagnostic => ({
      from: diagnostic.start ?? 0,
      message: ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n'),
      severity: diagnostic.category === ts.DiagnosticCategory.Error ? 'error' : 'warning',
      to: (diagnostic.start ?? 0) + (diagnostic.length ?? 0),
    }))
  },
  hover(pos, source) {
    sync(source)
    const info = env?.languageService.getQuickInfoAtPosition(CELL, pos)
    if (!info)
      return null
    return {
      doc: ts.displayPartsToString(info.documentation),
      from: info.textSpan.start,
      to: info.textSpan.start + info.textSpan.length,
      type: ts.displayPartsToString(info.displayParts),
    }
  },
  init,
  update(source) {
    sync(source)
  },
}

createBirpc<object, LanguageWorkerApi>(api, {
  on: fn => addEventListener('message', event => fn(event.data)),
  post: data => postMessage(data),
})
