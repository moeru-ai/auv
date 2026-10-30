import { fileURLToPath } from 'node:url'

import ts from 'typescript'

import { describe, expect, it } from 'vitest'

import { describeSdk } from './documentation'

const driverPath = fileURLToPath(new URL('../src/apis/auv/driver.ts', import.meta.url))
const sources: string[] = []
const docs = describeSdk({
  omitMethods: ['WindowClient.scrollMotion', 'WindowClient.scrollStream', 'WindowClient.scrollWith', 'runner.input.moveMouse'],
  onSource: path => sources.push(path.replaceAll('\\', '/')),
  runner: 'runner',
})
const definitions = Object.values(docs.types).map(type => type.definition).join('\n')

describe('sdk documentation', () => {
  it('describes client methods under the host path with resolved signatures', () => {
    const findText = docs.entries.find(entry => entry.path === 'WindowClient.findText')
    expect(findText?.signature).toBe('WindowClient.findText(query: string, options?: undefined | FindWindowTextOptions): Promise<FindWindowTextResponse>')
    expect(findText?.description).not.toBe('')
    expect(docs.types.FindWindowTextResponse.definition).toMatch(/\bmatches\??:/)
    expect(docs.entries.some(entry => entry.path === 'runner.windows.resolve')).toBe(true)
    expect(docs.entries.some(entry => entry.path === 'WindowClient.id')).toBe(false)
    expect(docs.types.CaptureTarget).toBeDefined()
    expect(sources.some(path => path.endsWith('/sdk/src/apis/auv/driver.ts'))).toBe(true)
  })

  // ROOT CAUSE:
  //
  // If an enum field was optional, TypeScript flattened `undefined | MouseButton`
  // into member literals, which printed as `import(".").MouseButton.LEFT`.
  //
  // Before the fix, those definitions were not valid TypeScript.
  // The fix names each member through its enum, which has a valued definition.
  it('names flattened enum members through their valued enum', () => {
    expect(definitions).toMatch(/button\?: undefined \| MouseButton\.UNSPECIFIED \| MouseButton\.LEFT/)
    expect(docs.types.MouseButton.definition).toMatch(/^enum MouseButton \{/)
    expect(definitions).not.toContain('import(')
  })

  // https://github.com/moeru-ai/auv/pull/414#discussion_r4399100615
  it('expands SDK and Protobuf helper aliases into concrete, valid definitions', () => {
    expect(docs.types.ScrollUntilOptions.definition).toContain('maxSteps?: undefined | number')
    expect(docs.types.ScrollUntilOptions.definition).toMatch(/settle\?: undefined \| number \|/)
    expect(definitions).toContain('case: "instant"')
    expect(definitions).toContain('case: "cubicBezier"')
    expect(definitions).toMatch(/enum ImageEncoding \{[^}]*PNG = 2,/)
    expect(Object.keys(docs.types)).not.toContain('MessageInit')
    expect(definitions).not.toMatch(/\b(?:Init|InputFields|MessageInit|Shape|WithMillis)</)
    expect(definitions).not.toMatch(/\$typeName|\$unknown/)
    const file = ts.createSourceFile('docs.ts', `export {};\n${definitions}`, ts.ScriptTarget.Latest)
    const options = { skipLibCheck: true, strict: true, target: ts.ScriptTarget.ESNext }
    const host = ts.createCompilerHost(options)
    const program = ts.createProgram([file.fileName], options, { ...host, getSourceFile: (name, ...args) => name === file.fileName ? file : host.getSourceFile(name, ...args) })
    const diagnostics = [...program.getSyntacticDiagnostics(file), ...program.getSemanticDiagnostics(file)]
    expect(diagnostics.map(item => ts.flattenDiagnosticMessageText(item.messageText, '\n'))).toEqual([])
  })

  it('leaves omitted methods out of entries and resource definitions', () => {
    expect(docs.entries.some(entry => entry.path === 'WindowClient.scrollMotion')).toBe(false)
    expect(docs.entries.some(entry => entry.path === 'runner.input.moveMouse')).toBe(false)
    expect(docs.types.WindowClient.definition).not.toContain('scrollMotion')
    expect(docs.types.WindowClient.definition).toContain('findText(')
    expect(() => describeSdk({ omitMethods: ['WindowClient.notReal'], runner: 'runner' })).toThrow('Cannot omit unknown SDK methods: WindowClient.notReal')
  })

  it('binds method paths to the host while preserving SDK examples', () => {
    const host = describeSdk({ runner: 'auv' })
    const resolve = host.entries.find(entry => entry.path === 'auv.windows.resolve')
    expect(resolve?.signature).toMatch(/^auv\.windows\.resolve\(/)
    expect(resolve?.examples).toEqual(docs.entries.find(entry => entry.path === 'runner.windows.resolve')?.examples)
    expect(resolve?.examples[0]).toContain('await runner.windows.resolve(')
  })

  it('describes selected public exports without reading host source', () => {
    const selected = describeSdk({ helpers: ['region', 'Position', 'InputPolicy'], runner: 'runner' })
    const entries = selected.entries.filter(entry => entry.path.startsWith('sdk.'))
    expect(entries.map(entry => entry.path)).toEqual(['sdk.region', 'sdk.Position.display', 'sdk.Position.screen', 'sdk.Position.window', 'sdk.InputPolicy'])
    expect(entries[0].description).not.toBe('')
    expect(entries[0].signature).toContain('rect: RectLike')
    expect(selected.types.InputPolicy.definition).toContain('BACKGROUND_ONLY = 1')
    expect(() => describeSdk({ helpers: ['notReal'], runner: 'runner' })).toThrow('Cannot find SDK export: notReal')
  })

  // Examples are documentation that an agent copies, so they must compile against the contract they describe.
  it('type-checks every example against the SDK', () => {
    const examples = docs.entries.flatMap(entry => entry.examples.map(example => ({ example, path: entry.path })))
    expect(examples.length).toBeGreaterThan(10)
    const name = driverPath.replace(/driver\.ts$/, 'examples.check.ts')
    const text = [
      'import type { RunnerClient } from \'./driver\'',
      'declare const runner: RunnerClient',
      ...examples.map(({ example }, index) => `async function example${index}() {\n${example}\n}\nvoid example${index}`),
    ].join('\n')
    const options: ts.CompilerOptions = { customConditions: ['browser'], module: ts.ModuleKind.ESNext, moduleResolution: ts.ModuleResolutionKind.Bundler, noEmit: true, skipLibCheck: true, strict: true, target: ts.ScriptTarget.ESNext }
    const host = ts.createCompilerHost(options)
    // The program passes module-detection options to `getSourceFile`; a file made without them is not a module.
    const program = ts.createProgram([name], options, { ...host, fileExists: path => path === name || host.fileExists(path), getSourceFile: (path, languageVersion, ...args) => path === name ? ts.createSourceFile(path, text, languageVersion) : host.getSourceFile(path, languageVersion, ...args), readFile: path => path === name ? text : host.readFile(path) })
    const file = program.getSourceFile(name)!
    const diagnostics = [...program.getSyntacticDiagnostics(file), ...program.getSemanticDiagnostics(file)]
    expect(diagnostics.map(item => ts.flattenDiagnosticMessageText(item.messageText, '\n'))).toEqual([])
  })
})
