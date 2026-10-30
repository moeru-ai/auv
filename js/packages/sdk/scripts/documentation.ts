import { fileURLToPath } from 'node:url'

import ts from 'typescript'

const driverPath = fileURLToPath(new URL('../src/apis/auv/driver.ts', import.meta.url))
const indexPath = fileURLToPath(new URL('../src/web/index.ts', import.meta.url))

export interface SdkDocumentation {
  entries: SdkEntry[]
  types: Record<string, SdkType>
}

export interface SdkDocumentationOptions {
  /** Public SDK export names to document under `sdk`. */
  helpers?: readonly string[]
  /** Fields the host cannot pass, such as callbacks. */
  omitFields?: readonly string[]
  /** Method paths the host cannot call, such as `WindowClient.scrollStream`. They leave entries and resource definitions. */
  omitMethods?: readonly string[]
  onSource?: (path: string) => void
  /** Path prefix of the `RunnerClient` methods, for example a host global name. */
  runner: string
}

/** One callable path, such as `WindowClient.findText`. */
export interface SdkEntry {
  description: string
  /** Source `@example` tags, in order. */
  examples: string[]
  path: string
  relatedTypes: string[]
  signature: string
}

export interface SdkType {
  definition: string
  description: string
  relatedTypes: string[]
}

// Protobuf-ES message metadata, not caller input.
const messageFields = ['$typeName', '$unknown']
// SDK and Protobuf-ES helper aliases. Their instantiations get readable names instead.
const helperAliases = ['Init', 'InputFields', 'MessageInit', 'Omit', 'Pick', 'Shape', 'WithMillis']

/**
 * Describe the SDK operation contract from source at build time: `RunnerClient`,
 * the `WindowClient` resource it returns, and `CaptureTarget`. Signatures and
 * reachable types are expanded to concrete fields, so a caller needs no SDK
 * helper alias to construct an input. Compiler objects never leave this function.
 */
// TODO(codemode-runner-docs): live RPC docs need an SDK-to-RPC mapping. Keep
// source-derived SDK docs until that mapping has an owner-approved consumer.
export function describeSdk(options: SdkDocumentationOptions): SdkDocumentation {
  const program = ts.createProgram([driverPath, ...(options.helpers ? [indexPath] : [])], {
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
    throw new Error(`Cannot generate accurate SDK docs. Regenerate the SDK bindings and resolve its type errors.\n${diagnostics.slice(0, 5).map(item => ts.flattenDiagnosticMessageText(item.messageText, '\n')).join('\n')}`)
  for (const source of program.getSourceFiles()) {
    if (source.fileName.replaceAll('\\', '/').includes('/sdk/src/'))
      options.onSource?.(source.fileName)
  }
  const omitted = new Set([...messageFields, ...(options.omitFields ?? [])])
  const omittedMethods = new Set(options.omitMethods)
  const seenMethods = new Set<string>()
  const types: Record<string, SdkType> = {}
  const names = new Map<ts.Type, string>()
  const pending: Array<{ name: string, type: ts.Type }> = []
  const refs = new Set<string>()
  const context = program.getSourceFile(driverPath)!
  const description = (symbol: ts.Symbol | undefined) => symbol ? ts.displayPartsToString(symbol.getDocumentationComment(checker)) : ''
  const safeName = (hint: string) => hint.replace(/[^a-z0-9]/gi, ' ').split(' ').filter(Boolean).map(word => word[0].toUpperCase() + word.slice(1)).join('')
  const objectFlags = (type: ts.Type) => type.flags & ts.TypeFlags.Object ? (type as ts.ObjectType).objectFlags : 0

  function render(type: ts.Type, hint: string): string {
    // Optional enum fields flatten into member literals, which print as module
    // imports. Name each member through its enum, which gets a valued definition.
    if (type.flags & ts.TypeFlags.EnumLiteral && !type.isUnion()) {
      const parent = checker.getBaseTypeOfLiteralType(type)
      if (parent !== type)
        return `${render(parent, hint)}.${type.getSymbol()!.name}`
    }
    if (type.flags & (ts.TypeFlags.Any | ts.TypeFlags.Unknown | ts.TypeFlags.Never | ts.TypeFlags.Void | ts.TypeFlags.Undefined | ts.TypeFlags.Null | ts.TypeFlags.StringLike | ts.TypeFlags.NumberLike | ts.TypeFlags.BooleanLike | ts.TypeFlags.BigIntLike))
      return checker.typeToString(type, context, ts.TypeFormatFlags.NoTruncation)
    const awaited = checker.getAwaitedType(type)
    if (awaited && awaited !== type)
      return `Promise<${render(awaited, `${hint}Result`)}>`
    if (checker.isArrayType(type))
      return `Array<${render(checker.getTypeArguments(type as ts.TypeReference)[0], `${hint}Item`)}>`
    if (type.isUnion() && !type.aliasSymbol && !(type.flags & ts.TypeFlags.EnumLiteral))
      return [...new Set(type.types.map(part => render(part, hint)))].join(' | ')
    // Standard library interfaces and classes such as `Uint8Array` and `AbortSignal`
    // are named, not expanded: their members describe the host, not the SDK.
    // Library aliases such as `Omit` still resolve to SDK fields below.
    const declared = type.getSymbol()
    if (declared && declared.flags & (ts.SymbolFlags.Interface | ts.SymbolFlags.Class) && declared.declarations?.some(item => program.isSourceFileDefaultLibrary(item.getSourceFile()))) {
      // Interface references can carry a trailing `this` argument; keep declared parameters only.
      const reference = objectFlags(type) & ts.ObjectFlags.Reference ? type as ts.TypeReference : undefined
      const args = reference ? checker.getTypeArguments(reference).slice(0, reference.target.typeParameters?.length ?? 0) : []
      return args.length ? `${declared.name}<${args.map(arg => render(arg, hint)).join(', ')}>` : declared.name
    }
    const existing = names.get(type)
    if (existing) {
      refs.add(existing)
      return existing
    }
    const symbol = type.aliasSymbol ?? type.getSymbol()
    const candidate = symbol?.name
    let name = candidate && !candidate.startsWith('__') && !helperAliases.includes(candidate) ? candidate : safeName(hint)
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

  const entries: SdkEntry[] = []
  function walk(properties: ts.Symbol[], path: string, depth = 0): void {
    if (depth > 4)
      throw new Error(`SDK docs nesting is too deep: ${path}`)
    for (const prop of properties) {
      const current = `${path}.${prop.name}`
      const declaration = prop.valueDeclaration ?? context
      const value = checker.getTypeOfSymbolAtLocation(prop, declaration)
      const call = value.getCallSignatures()[0]
      refs.clear()
      if (call) {
        seenMethods.add(current)
        if (omittedMethods.has(current))
          continue
        const text = `${current}${signature(call, current)}`
        // Re-exported helpers can carry JSDoc on the function rather than the export symbol.
        const doc = description(prop) || ts.displayPartsToString(call.getDocumentationComment(checker))
        const tags = [...prop.getJsDocTags(checker), ...call.getJsDocTags()]
        // Preserve source examples; the host supplies any environment-specific bindings.
        const examples = [...new Set(tags.filter(tag => tag.name === 'example').map(tag => ts.displayPartsToString(tag.text).trim()))]
        entries.push({ description: doc, examples, path: current, relatedTypes: [...refs], signature: text })
      }
      else if ((value.getSymbol()?.flags ?? 0) & ts.SymbolFlags.Enum) {
        // Enum objects expose numeric reverse mappings at runtime. Document
        // their source enum instead of advertising those mappings as methods.
        const text = render(value, prop.name)
        entries.push({ description: description(prop), examples: [], path: current, relatedTypes: [...refs], signature: `${current}: ${text}` })
      }
      else if (objectFlags(value) & ts.ObjectFlags.Anonymous) {
        // Namespace objects such as `runner.windows`. Resource data such as
        // `WindowClient.id` stays in the type definition only.
        walk(checker.getPropertiesOfType(value), current, depth + 1)
      }
    }
  }

  for (const node of context.statements) {
    if (ts.isInterfaceDeclaration(node) && ['RunnerClient', 'WindowClient'].includes(node.name.text))
      walk(checker.getPropertiesOfType(checker.getTypeAtLocation(node)), node.name.text === 'RunnerClient' ? options.runner : 'WindowClient')
    if (ts.isTypeAliasDeclaration(node) && node.name.text === 'CaptureTarget')
      render(checker.getTypeAtLocation(node), 'CaptureTarget')
  }
  if (options.helpers) {
    const exports = checker.getExportsOfModule(checker.getSymbolAtLocation(program.getSourceFile(indexPath)!)!)
    walk(options.helpers.map((name) => {
      const symbol = exports.find(item => item.name === name)
      if (!symbol)
        throw new Error(`Cannot find SDK export: ${name}`)
      return symbol
    }), 'sdk')
  }
  const unknownMethods = [...omittedMethods].filter(path => !seenMethods.has(path))
  if (unknownMethods.length)
    throw new Error(`Cannot omit unknown SDK methods: ${unknownMethods.join(', ')}`)

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
      const fields = checker.getPropertiesOfType(type).filter(prop => !omitted.has(prop.name))
      const members = fields.map((prop) => {
        const declaration = prop.valueDeclaration ?? context
        const value = checker.getTypeOfSymbolAtLocation(prop, declaration)
        const call = value.getCallSignatures()[0]
        if (call && omittedMethods.has(`${name}.${prop.name}`))
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
      throw new Error(`SDK docs unexpectedly reached compiler or host object types at ${name}: ${pending.slice(-10).map(item => item.name).join(', ')}`)
  }
  return { entries, types }
}
