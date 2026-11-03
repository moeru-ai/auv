import type { Plugin } from 'vite'

import type { ApiCatalog, ApiEntry, ApiType } from '../src/script-api/codemode-help'

import { fileURLToPath } from 'node:url'

import ts from 'typescript'

const driverPath = fileURLToPath(new URL('../../../js/packages/sdk/src/apis/auv/driver.ts', import.meta.url))
const globalsPath = fileURLToPath(new URL('../src/script-api/codemode-sdk.ts', import.meta.url))
const virtualId = 'virtual:auv-codemode-help'

const unavailable: Record<string, string> = {
  'auv.captures.image': 'Raw image bytes do not cross the VM. Use await image(capture, options) for bounded image output.',
  'auv.input.moveMouse': 'This method returns an async iterator, which cannot cross the VM. Pointer clicks are available through auv.input.click or WindowClient.click.',
  'auv.input.streamMouseMotion': 'Duplex calls and async iterators cannot cross the VM.',
  'WindowClient.scrollMotion': 'Async iterators cannot cross the VM. Use WindowClient.scroll instead.',
  'WindowClient.scrollStream': 'The returned controller contains an async iterator. Use WindowClient.scroll instead.',
  'WindowClient.scrollWith': 'Generator inputs cannot cross the VM. Use WindowClient.scroll or scrollUntil without callbacks.',
}

const examples: Record<string, string[]> = {
  'auv.displays.capture': ['const displays = await auv.displays.list(); const display = displays.at(-1); const selector = { selector: { case: "display", value: { displayId: display.displayId } } }; await image((await auv.displays.capture(selector)).capture)'],
  'auv.displays.findText': ['const displays = await auv.displays.list(); text(await auv.displays.findText({ selector: { case: "display", value: { displayId: displays.at(-1).displayId } } }, "Counter"))'],
  'auv.input.keyboard': ['const win = await auv.windows.resolve({ appName: "Todos" }); await win.click({ x: 100, y: 74 }); text(await auv.input.keyboard({ target: { recipient: { case: "window", value: { ref: { windowId: win.id } } } }, inputs: [{ action: { case: "typeText", value: { text: "Buy milk" } } }, { action: { case: "press", value: { options: { keys: ["Return"] } } } }] }))'],
  'auv.recognizeText': ['const win = await auv.windows.resolve({ appName: "Counter" }); const shot = await win.capture(); text(await auv.recognizeText(shot.capture))'],
  'auv.windows.from': ['const win = auv.windows.from(load("windowId")); text(await win.findText("Count"))'],
  'auv.windows.get': ['const win = await auv.windows.get(load("windowId")); text(win.window)'],
  'auv.windows.list': ['text((await auv.windows.list()).map(win => ({ id: win.id, window: win.window })))'],
  'auv.windows.resolve': ['const win = await auv.windows.resolve({ appName: "Counter" }); text(win.id)'],
  'sdk.Position.display': ['text(sdk.Position.display("display-id", 10, 20))'],
  'sdk.Position.screen': ['text(sdk.Position.screen(100, 200))'],
  'sdk.Position.window': ['const win = await auv.windows.resolve({ appName: "Counter" }); text(sdk.Position.window(win.id, 10, 20))'],
  'sdk.region': ['text(sdk.region({ x: 10, y: 20, width: 400, height: 300 }, { left: "50%", width: "50%" }))'],
  'WindowClient.capture': ['const win = await auv.windows.resolve({ appName: "Counter" }); const shot = await win.capture(); await image(shot.capture)'],
  'WindowClient.click': ['const win = await auv.windows.resolve({ appName: "Counter" }); const found = await win.findText("Increment"); if (found.matches.length !== 1) throw new Error("Expected one match"); await win.click(found.matches[0]); text((await win.findText("Count")).matches)'],
  'WindowClient.findText': ['const win = await auv.windows.resolve({ appName: "Counter" }); text((await win.findText("Increment")).matches)'],
  'WindowClient.pasteText': ['const win = await auv.windows.resolve({ appName: "Todos" }); await win.pasteText("Buy milk")'],
  'WindowClient.pressKeys': ['const win = await auv.windows.resolve({ appName: "Todos" }); await win.pressKeys("return")'],
  'WindowClient.scroll': ['const win = await auv.windows.resolve({ appName: "Music" }); await win.scroll({ x: 100, y: 200 }, { deltaY: 400 })'],
  'WindowClient.scrollUntil': ['const win = await auv.windows.resolve({ appName: "Music" }); text(await win.scrollUntil({ x: 100, y: 200 }, { step: { case: "instant", value: { deltaY: 400 } }, maxSteps: 5 }))'],
  'WindowClient.typeText': ['const win = await auv.windows.resolve({ appName: "Todos" }); await win.typeText("Buy milk")'],
}

/** Project SDK types at build time. Compiler objects and filesystem paths never enter the VM. */
// TODO(codemode-runner-docs): live RPC docs need an SDK-to-RPC mapping. Keep
// source-derived SDK help until that mapping has an owner-approved consumer.
export function buildHelpCatalog(onSource?: (path: string) => void): ApiCatalog {
  const program = ts.createProgram([driverPath, globalsPath], {
    customConditions: ['browser'],
    module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    skipLibCheck: true,
    strict: true,
    target: ts.ScriptTarget.ESNext,
  })
  const checker = program.getTypeChecker()
  const diagnostics = program.getSemanticDiagnostics().filter(item => item.category === ts.DiagnosticCategory.Error && item.file?.fileName.replaceAll('\\', '/').includes('/sdk/src/'))
  if (diagnostics.length)
    throw new Error(`Cannot generate accurate SDK help. Regenerate the SDK bindings and resolve its type errors.\n${diagnostics.slice(0, 5).map(item => ts.flattenDiagnosticMessageText(item.messageText, '\n')).join('\n')}`)
  for (const source of program.getSourceFiles()) {
    if (source.fileName.replaceAll('\\', '/').includes('/sdk/src/'))
      onSource?.(source.fileName)
  }
  onSource?.(globalsPath)
  const types: Record<string, ApiType> = {}
  const names = new Map<ts.Type, string>()
  const pending: Array<{ name: string, type: ts.Type }> = []
  const refs = new Set<string>()
  const context = program.getSourceFile(driverPath)!
  const description = (symbol: ts.Symbol | undefined) => symbol ? ts.displayPartsToString(symbol.getDocumentationComment(checker)) : ''
  const safeName = (hint: string) => hint.replace(/[^a-z0-9]/gi, ' ').split(' ').filter(Boolean).map(word => word[0].toUpperCase() + word.slice(1)).join('')

  function render(type: ts.Type, hint: string): string {
    if (['ArrayBuffer', 'ArrayBufferLike', 'Uint8Array'].includes(type.getSymbol()?.name ?? ''))
      return checker.typeToString(type, context)
    if (type.flags & (ts.TypeFlags.Any | ts.TypeFlags.Unknown | ts.TypeFlags.Never | ts.TypeFlags.Void | ts.TypeFlags.Undefined | ts.TypeFlags.Null | ts.TypeFlags.StringLike | ts.TypeFlags.NumberLike | ts.TypeFlags.BooleanLike | ts.TypeFlags.BigIntLike))
      return checker.typeToString(type, context, ts.TypeFormatFlags.NoTruncation)
    const awaited = checker.getAwaitedType(type)
    if (awaited && awaited !== type)
      return `Promise<${render(awaited, `${hint}Result`)}>`
    if (checker.isArrayType(type))
      return `Array<${render(checker.getTypeArguments(type as ts.TypeReference)[0], `${hint}Item`)}>`
    if (type.isUnion() && !type.aliasSymbol)
      return [...new Set(type.types.map(part => render(part, hint)))].join(' | ')
    const existing = names.get(type)
    if (existing) {
      refs.add(existing)
      return existing
    }
    const symbol = type.aliasSymbol ?? type.getSymbol()
    const candidate = symbol?.name
    let name = candidate && !candidate.startsWith('__') && !['Init', 'InputFields', 'Omit', 'Pick', 'Shape', 'WithMillis'].includes(candidate) ? candidate : safeName(hint)
    const base = name
    let suffix = 2
    while (types[name]) name = `${base}${suffix++}`
    names.set(type, name)
    refs.add(name)
    types[name] = { definition: '', description: description(symbol), relatedTypes: [] }
    pending.push({ name, type })
    return name
  }

  function signature(signature: ts.Signature, path: string): string {
    const params = signature.getParameters().map((param) => {
      const declaration = param.valueDeclaration ?? signature.declaration ?? context
      const optional = !!(param.flags & ts.SymbolFlags.Optional) || (ts.isParameter(declaration) && !!(declaration.questionToken || declaration.initializer))
      return `${param.name}${optional ? '?' : ''}: ${render(checker.getTypeOfSymbolAtLocation(param, declaration), `${path} ${param.name}`)}`
    })
    return `(${params.join(', ')}): ${render(checker.getReturnTypeOfSignature(signature), `${path} result`)}`
  }

  const entries: ApiEntry[] = []
  function walk(type: ts.Type, path: string, depth = 0): void {
    if (depth > 4)
      throw new Error(`SDK help nesting is too deep: ${path}`)
    for (const prop of checker.getPropertiesOfType(type)) {
      const current = `${path}.${prop.name}`
      const declaration = prop.valueDeclaration ?? context
      const value = checker.getTypeOfSymbolAtLocation(prop, declaration)
      const call = value.getCallSignatures()[0]
      refs.clear()
      if (call) {
        const returned = checker.getAwaitedType(checker.getReturnTypeOfSignature(call))!
        const restriction = unavailable[current] ?? (/\b(?:AsyncIterable|AsyncGenerator|TypedDuplexCall)\b/.test(checker.typeToString(returned)) ? 'Streaming results cannot cross the VM yet.' : undefined)
        const text = `${current}${restriction ? checker.signatureToString(call, context) : signature(call, current)}`
        entries.push({ description: description(prop), examples: examples[current] ?? [], path: current, relatedTypes: [...refs], signature: text, ...(restriction ? { unavailable: restriction } : {}) })
      }
      else if (path === 'sdk' && prop.name !== 'Position') {
        // Enum objects expose numeric reverse mappings at runtime. Document
        // their source enum instead of advertising those mappings as methods.
        const text = render(value, prop.name)
        entries.push({ description: description(prop), examples: [], path: current, relatedTypes: [...refs], signature: `${current}: ${text}` })
      }
      else if (path.startsWith('auv') || current === 'sdk.Position') {
        walk(value, current, depth + 1)
      }
    }
  }

  const driver = program.getSourceFile(driverPath)!
  for (const node of driver.statements) {
    if (ts.isInterfaceDeclaration(node) && ['RunnerClient', 'WindowClient'].includes(node.name.text))
      walk(checker.getTypeAtLocation(node), node.name.text === 'RunnerClient' ? 'auv' : 'WindowClient')
    if (ts.isTypeAliasDeclaration(node) && node.name.text === 'CaptureTarget')
      render(checker.getTypeAtLocation(node), 'CaptureTarget')
  }
  const globals = program.getSourceFile(globalsPath)!
  for (const node of globals.statements) {
    if (ts.isVariableStatement(node)) {
      for (const declaration of node.declarationList.declarations) {
        if (declaration.name.getText() === 'codeModeSdk')
          walk(checker.getTypeAtLocation(declaration), 'sdk')
      }
    }
  }

  for (let index = 0; index < pending.length; index++) {
    const { name, type } = pending[index]
    refs.clear()
    // Enum unions otherwise print member imports without their wire values.
    const enumDeclaration = type.getSymbol()?.declarations?.find(ts.isEnumDeclaration)
    if (enumDeclaration) {
      const members = enumDeclaration.members.map(member => `  ${member.name.getText()} = ${checker.getConstantValue(member)},`)
      types[name].definition = `enum ${name} {\n${members.join('\n')}\n}`
      continue
    }
    let definition: string
    if (type.isUnion()) {
      definition = type.types.map(part => render(part, `${name}Value`)).join(' | ')
    }
    else {
      const fields = checker.getPropertiesOfType(type).filter(prop => !['$typeName', '$unknown', 'onUpdate', 'signal', 'until'].includes(prop.name))
      const members = fields.map((prop) => {
        const declaration = prop.valueDeclaration ?? context
        const value = checker.getTypeOfSymbolAtLocation(prop, declaration)
        const call = value.getCallSignatures()[0]
        if (call && name === 'WindowClient' && unavailable[`WindowClient.${prop.name}`])
          return ''
        const doc = description(prop).split(/\r?\n/)[0].slice(0, 200)
        const constant = ts.isEnumMember(declaration) ? checker.getConstantValue(declaration) : undefined
        const valueText = constant !== undefined ? `: ${JSON.stringify(constant)}` : call ? signature(call, `${name} ${prop.name}`) : `: ${render(value, `${name} ${prop.name}`)}`
        return `${doc ? `  /** ${doc} */\n` : ''}  ${prop.name}${prop.flags & ts.SymbolFlags.Optional ? '?' : ''}${valueText};`
      }).filter(Boolean)
      definition = `{\n${members.join('\n')}\n}`
    }
    types[name].definition = `type ${name} = ${definition}`
    types[name].relatedTypes = [...refs].filter(ref => ref !== name)
    if (pending.length > 1500)
      throw new Error(`SDK help unexpectedly reached compiler or host object types at ${name}: ${pending.slice(-10).map(item => item.name).join(', ')}`)
  }

  entries.push(
    { description: 'Emit selected text or JSON data without functions. For WindowClient, output its id or window metadata. Only explicit output returns to the agent.', examples: ['text({ found: 1 })', ...examples['auv.windows.list']], path: 'text', relatedTypes: [], signature: 'text(value: unknown): void' },
    { description: 'Fetch a capture reference as bounded PNG/JPEG output. Pixels stay outside VM memory.', examples: examples['WindowClient.capture'], path: 'image', relatedTypes: ['CaptureTarget'], signature: 'image(capture: CaptureTarget, options?: { maxSize?: { width: number, height: number }, encoding?: sdk.ImageEncoding.PNG | sdk.ImageEncoding.JPEG }): Promise<void>' },
    { description: 'Keep a small JSON value after successful execution. Use undefined to remove a key.', examples: ['store("windowId", (await auv.windows.resolve({ appName: "Counter" })).id)'], path: 'store', relatedTypes: [], signature: 'store(key: string, value: unknown): void' },
    { description: 'Read a saved JSON value. Returns undefined for a missing key.', examples: ['text(load("windowId"))'], path: 'load', relatedTypes: [], signature: 'load(key: string): unknown' },
  )
  return { entries, types }
}

/** One metadata snapshot for the page, worker, and tests. SDK edits invalidate it. */
export function codeModeHelpPlugin(): Plugin {
  let catalog: ApiCatalog | undefined
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
      catalog ??= buildHelpCatalog(path => sources.add(path.replaceAll('\\', '/')))
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
