import type { SdkDocumentation, SdkEntry } from '@auv-js/sdk/documentation'
import type { Plugin } from 'vite'

import { describeSdk } from '@auv-js/sdk/documentation'

import { codeModeExports } from '../src/script-api/codemode-exports'

const virtualId = 'virtual:auv-codemode-help'

// Streams, generators, and raw pixels cannot cross the VM, so help omits them.
// A guessed call is rejected by the execution bridge. The test for this list
// checks that no remaining definition mentions a stream or generator type.
const omitMethods = [
  'auv.captures.image',
  'auv.input.moveMouse',
  'auv.input.streamMouseMotion',
  'WindowClient.scrollMotion',
  'WindowClient.scrollStream',
  'WindowClient.scrollWith',
]

// Examples of the SDK calls live in the SDK's JSDoc. These globals exist only in this VM.
const globals: SdkEntry[] = [
  { description: 'Emit selected text or JSON data without functions. For WindowClient, output its id or window metadata. Only explicit output returns to the agent.', examples: ['text({ found: 1 })', 'text((await auv.windows.list()).map(win => ({ id: win.id, window: win.window })))'], path: 'text', relatedTypes: [], signature: 'text(value: unknown): void' },
  { description: 'Fetch a capture reference as bounded PNG/JPEG output. Pixels stay outside VM memory, so auv.captures.image is not available.', examples: ['const win = await auv.windows.resolve({ appName: "Counter" }); const shot = await win.capture(); await image(shot.capture)'], path: 'image', relatedTypes: ['CaptureTarget'], signature: 'image(capture: CaptureTarget, options?: { maxSize?: { width: number, height: number }, encoding?: sdk.ImageEncoding.PNG | sdk.ImageEncoding.JPEG }): Promise<void>' },
  { description: 'Keep a small JSON value after successful execution. Use undefined to remove a key.', examples: ['store("windowId", (await auv.windows.resolve({ appName: "Counter" })).id)'], path: 'store', relatedTypes: [], signature: 'store(key: string, value: unknown): void' },
  { description: 'Read a saved JSON value. Returns undefined for a missing key.', examples: ['text(load("windowId"))'], path: 'load', relatedTypes: [], signature: 'load(key: string): unknown' },
]

/** One metadata snapshot for the page, worker, and tests. SDK edits invalidate it. */
export function codeModeHelpPlugin(): Plugin {
  let catalog: SdkDocumentation | undefined
  const sources = new Set<string>()
  return {
    handleHotUpdate(context) {
      if (!sources.has(context.file.replaceAll('\\', '/')))
        return
      catalog = undefined
      const module = context.server.moduleGraph.getModuleById(`\0${virtualId}`)
      if (!module)
        return
      context.server.moduleGraph.invalidateModule(module)
      return [...context.modules, module]
    },
    load(id) {
      if (id !== `\0${virtualId}`)
        return
      if (!catalog) {
        // Add VM policy and globals to the SDK contract. Compiler objects never enter the VM.
        catalog = describeSdk({
          helpers: codeModeExports,
          // Callbacks and AbortSignal cannot cross the VM; the host cancels the whole execution.
          omitFields: ['onUpdate', 'signal', 'until'],
          omitMethods,
          onSource: path => sources.add(path.replaceAll('\\', '/')),
          runner: 'auv',
        })
        // SDK examples use `runner`; bind it to this VM's client without rewriting source code.
        for (const entry of catalog.entries)
          entry.examples = entry.examples.map(example => `const runner = auv;\n${example}`)
        catalog.entries.push(...globals)
      }
      for (const source of sources) this.addWatchFile(source)
      return `export default ${JSON.stringify(catalog)}`
    },
    name: 'auv-codemode-help',
    resolveId: id => id === virtualId ? `\0${virtualId}` : undefined,
    watchChange(id) {
      if (sources.has(id.replaceAll('\\', '/')))
        catalog = undefined
    },
  }
}
