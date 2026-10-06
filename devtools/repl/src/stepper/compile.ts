import type { Node, Pattern, Program } from 'acorn'

import MagicString from 'magic-string'
import tsBlankSpace from 'ts-blank-space'

import { parse } from 'acorn'

/** A site that reports the values of freshly bound names. */
export interface BindSite {
  id: number
  /** Editor line of the declaration or loop header. */
  line: number
  names: string[]
}

export interface CompiledCell {
  binds: BindSite[]
  /** Instrumented script that evaluates to a promise of the completion value. */
  code: string
  /** Top-level names that persist across cells as global properties. */
  hoisted: string[]
  /** `var` prelude that must run (indirect eval) before `code`. */
  prelude: string
  steps: StepSite[]
}

export interface CompileOptions {
  /** Stable cell identifier used in `sourceURL` and stack traces. */
  cellId?: number | string
  /**
   * Editor line where `source` starts. Running a selection or a single line
   * keeps editor line numbers by padding the source with blank lines.
   */
  startLine?: number
}

/** One pausable or record-only hook site in a compiled cell. */
export interface StepSite {
  /** Whether the hook can await (async context) or only record (sync function). */
  async: boolean
  /** Last source line covered by the statement (1-based, editor coordinates). */
  endLine: number
  id: number
  /** First source line of the statement (1-based, editor coordinates). */
  line: number
  /** Whether the statement is a top-level statement of the cell. */
  top: boolean
}

type AnyNode = Node & Record<string, unknown> & { loc: { end: { line: number }, start: { line: number } } }

interface VisitContext { async: boolean, top: boolean }

export class CellSyntaxError extends SyntaxError {
  constructor(message: string, readonly line?: number, readonly column?: number) {
    super(message)
    this.name = 'CellSyntaxError'
  }
}

const FUNCTION_TYPES = new Set(['ArrowFunctionExpression', 'FunctionDeclaration', 'FunctionExpression'])
const LOOP_TYPES = new Set(['ForInStatement', 'ForOfStatement', 'ForStatement'])

/**
 * Compiles one TypeScript REPL cell into stepped JavaScript.
 *
 * Pipeline: TS --(ts-blank-space)--> JS with identical offsets --(acorn)-->
 * ESTree --(magic-string)--> stepped + REPL-hoisted cell code. Because
 * ts-blank-space preserves every offset, acorn's locations on the stripped
 * JavaScript are the original TypeScript lines; no source-map chaining is
 * needed for line highlighting.
 *
 * The compiled code expects these globals in its realm:
 * - `__step(id)`: async hook before statements in async contexts; the host
 *   may keep the returned promise pending to pause.
 * - `__stepSync(id)`: record-only hook inside sync functions.
 * - `__bind(id, values)`: reports the values of names bound by a declaration
 *   (after it runs) or by a loop header (at the start of each iteration).
 */
export function compileCell(source: string, options: CompileOptions = {}): CompiledCell {
  const startLine = Math.max(1, options.startLine ?? 1)
  const padded = '\n'.repeat(startLine - 1) + source
  const unsupported: string[] = []
  const js = tsBlankSpace(padded, node => unsupported.push(String(node.kind)))
  if (unsupported.length > 0) {
    // NOTICE: ts-blank-space only erases type syntax. enums, namespaces and
    // constructor parameter properties need real emit and are rejected, like
    // Node's strip-only TypeScript mode.
    throw new CellSyntaxError('Unsupported TypeScript syntax: enums, namespaces and parameter properties need emit and are not supported in cells.')
  }

  let ast: Program
  try {
    ast = parse(js, {
      allowAwaitOutsideFunction: true,
      ecmaVersion: 'latest',
      locations: true,
      sourceType: 'script',
    })
  }
  catch (error) {
    const loc = (error as { loc?: { column: number, line: number } }).loc
    throw new CellSyntaxError((error as Error).message, loc?.line, loc?.column)
  }

  const ms = new MagicString(js)
  const steps: StepSite[] = []
  const binds: BindSite[] = []
  const hoisted = new Set<string>()
  const functionExports: string[] = []
  // Declaration binds are appended after the hoisting rewrite so they land
  // after its closing parens at the same offset.
  const pendingBinds: { at: number, text: string }[] = []

  const stepCall = (node: AnyNode, ctx: VisitContext): string => {
    const id = steps.length
    steps.push({ async: ctx.async, endLine: node.loc.end.line, id, line: node.loc.start.line, top: ctx.top })
    // NOTICE: sync functions and sync generators cannot `await`; they get a
    // record-only hook, so the host can highlight but not pause there.
    return ctx.async ? `await __step(${id});` : `__stepSync(${id});`
  }

  const bindCall = (names: string[], line: number): string => {
    const id = binds.length
    binds.push({ id, line, names })
    return `__bind(${id}, { ${names.join(', ')} });`
  }

  /** Names a loop header binds per iteration, e.g. `const x` in `for (const x of xs)`. */
  const loopNames = (loop: AnyNode): string[] => {
    const head = (loop.type === 'ForStatement' ? loop.init : loop.left) as AnyNode | null
    if (head?.type !== 'VariableDeclaration')
      return []
    return (head.declarations as AnyNode[]).flatMap(declaration => collectNames(declaration.id as unknown as Pattern))
  }

  const visitList = (list: AnyNode[], ctx: VisitContext): void => {
    for (const statement of list) {
      if (statement.type === 'FunctionDeclaration' || statement.type === 'EmptyStatement') {
        visit(statement, ctx)
        continue
      }
      // Keep directive prologues ("use strict") first in their body.
      if (statement.type === 'ExpressionStatement' && statement.directive)
        continue
      ms.prependLeft(statement.start, `${stepCall(statement, ctx)} `)
      if (statement.type === 'VariableDeclaration') {
        const names = (statement.declarations as AnyNode[]).flatMap(declaration => collectNames(declaration.id as unknown as Pattern))
        if (names.length > 0)
          pendingBinds.push({ at: statement.end, text: `; ${bindCall(names, statement.loc.start.line)}` })
      }
      visit(statement, { ...ctx, top: false })
    }
  }

  // `if (x) stmt` / `for (...) stmt` bodies are single statements; wrap them so
  // the hook and the statement share the body position. `prefix` runs first
  // (loop iteration binds).
  const visitBody = (statement: AnyNode | null | undefined, ctx: VisitContext, prefix = ''): void => {
    if (!statement)
      return
    if (statement.type === 'BlockStatement') {
      if (prefix)
        ms.appendRight(statement.start + 1, ` ${prefix}`)
      visitList(statement.body as AnyNode[], ctx)
      return
    }
    ms.prependLeft(statement.start, `{ ${prefix} ${stepCall(statement, ctx)} `)
    ms.appendRight(statement.end, ' }')
    visit(statement, ctx)
  }

  const visitFunction = (fn: AnyNode): void => {
    const ctx: VisitContext = { async: Boolean(fn.async), top: false }
    const body = fn.body as AnyNode
    if (body.type === 'BlockStatement') {
      visitList(body.body as AnyNode[], ctx)
      return
    }
    // Expression-bodied async arrow: rewrite into a block so it can pause.
    // NOTICE: acorn drops parens, so `async () => ({a})` starts its body at
    // `{`; wrap from just after `=>` so the parens move inside the return.
    if (fn.async) {
      const after = js.lastIndexOf('=>', body.start) + 2
      ms.prependLeft(after, ` { ${stepCall(body, ctx)} return (`)
      ms.appendLeft(fn.end, '); }')
    }
    // TODO(stepper-sync-arrow): sync expression-bodied arrows get no hook.
    // `(__stepSync(id), expr)` would record them; add it when per-callback
    // line timing is needed in the timeline.
    visit(body, ctx)
  }

  function visit(node: AnyNode | null | undefined, ctx: VisitContext): void {
    if (!node || typeof node.type !== 'string')
      return
    if (FUNCTION_TYPES.has(node.type)) {
      visitFunction(node)
      return
    }
    if (LOOP_TYPES.has(node.type)) {
      for (const key of ['init', 'test', 'update', 'left', 'right'])
        visit(node[key] as AnyNode | null, ctx)
      const names = loopNames(node)
      visitBody(node.body as AnyNode, ctx, names.length > 0 ? bindCall(names, node.loc.start.line) : '')
      return
    }
    switch (node.type) {
      case 'BlockStatement':
        visitList(node.body as AnyNode[], ctx)
        return
      case 'DoWhileStatement':
        visitBody(node.body as AnyNode, ctx)
        visit(node.test as AnyNode, ctx)
        return
      case 'IfStatement':
        visit(node.test as AnyNode, ctx)
        visitBody(node.consequent as AnyNode, ctx)
        visitBody(node.alternate as AnyNode | null, ctx)
        return
      // NOTICE: never wrap a labeled loop in a block; `continue label` needs
      // the label directly on the iteration statement.
      case 'LabeledStatement':
        visit(node.body as AnyNode, ctx)
        return
      case 'StaticBlock':
        visitList(node.body as AnyNode[], { async: false, top: false })
        return
      case 'SwitchCase':
        visit(node.test as AnyNode | null, ctx)
        visitList(node.consequent as AnyNode[], ctx)
        return
      case 'TryStatement':
        visit(node.block as AnyNode, ctx)
        if (node.handler)
          visit((node.handler as AnyNode).body as AnyNode, ctx)
        visit(node.finalizer as AnyNode | null, ctx)
        return
      case 'WhileStatement':
        visit(node.test as AnyNode, ctx)
        visitBody(node.body as AnyNode, ctx)
        return
    }
    for (const key of Object.keys(node)) {
      if (key === 'loc')
        continue
      const value = node[key]
      if (Array.isArray(value))
        value.forEach(child => visit(child as AnyNode, ctx))
      else if (value && typeof (value as AnyNode).type === 'string')
        visit(value as AnyNode, ctx)
    }
  }

  // Step hooks first: their closers must land before the hoisting closers.
  visitList(ast.body as AnyNode[], { async: true, top: true })

  // REPL persistence: rewrite top-level declarations into assignments to
  // prelude-declared globals (the node:repl `await.js` strategy). `const`
  // becomes reassignable across cells, matching node:repl.
  for (const statement of ast.body as AnyNode[]) {
    if (statement.type === 'VariableDeclaration') {
      const declarations = statement.declarations as AnyNode[]
      const multi = declarations.length > 1
      const kind = statement.kind as string
      ms.overwrite(statement.start, statement.start + kind.length, multi ? 'void (' : 'void', { contentOnly: true })
      for (const declaration of declarations) {
        collectNames(declaration.id as unknown as Pattern).forEach(name => hoisted.add(name))
        ms.prependRight(declaration.start, '(')
        ms.appendLeft(declaration.end, declaration.init ? ')' : '=undefined)')
      }
      if (multi)
        ms.appendLeft(declarations.at(-1)!.end, ')')
    }
    else if (statement.type === 'ClassDeclaration') {
      const name = (statement.id as unknown as { name: string }).name
      hoisted.add(name)
      ms.prependRight(statement.start, `${name} = `)
    }
    else if (statement.type === 'FunctionDeclaration') {
      const name = (statement.id as unknown as { name: string }).name
      hoisted.add(name)
      functionExports.push(`globalThis.${name} = ${name};`)
    }
  }

  for (const bind of pendingBinds)
    ms.appendLeft(bind.at, bind.text)

  // The last top-level expression statement becomes the cell's value.
  const last = ast.body.at(-1) as AnyNode | undefined
  if (last?.type === 'ExpressionStatement') {
    const expression = last.expression as AnyNode
    ms.prependRight(expression.start, 'return (')
    ms.appendLeft(expression.end, ')')
  }

  const file = `cell-${options.cellId ?? 1}.ts`
  // The wrapper prefix stays on line 1 so runtime line numbers equal editor lines.
  ms.prepend(`(async () => { ${functionExports.join(' ')} `)
  ms.append('\n})()')
  const map = ms.generateMap({ hires: 'boundary', includeContent: true, source: file })
  map.sourcesContent = [padded]
  const code = `${ms.toString()}\n//# sourceURL=${file}.js\n//# sourceMappingURL=data:application/json;base64,${toBase64(map.toString())}`
  const names = [...hoisted]
  return {
    binds,
    code,
    hoisted: names,
    prelude: names.length > 0 ? `var ${names.join(', ')};` : '',
    steps,
  }
}

function collectNames(pattern: null | Pattern | undefined, out: string[] = []): string[] {
  if (!pattern)
    return out
  switch (pattern.type) {
    case 'ArrayPattern':
      for (const element of pattern.elements)
        collectNames(element, out)
      break
    case 'AssignmentPattern':
      collectNames(pattern.left, out)
      break
    case 'Identifier':
      out.push(pattern.name)
      break
    case 'ObjectPattern':
      for (const property of pattern.properties)
        collectNames(property.type === 'RestElement' ? property.argument : property.value, out)
      break
    case 'RestElement':
      collectNames(pattern.argument, out)
      break
  }
  return out
}

function toBase64(text: string): string {
  const bytes = new TextEncoder().encode(text)
  let binary = ''
  for (const byte of bytes)
    binary += String.fromCharCode(byte)
  return btoa(binary)
}
