import type { CompiledCell } from './compile'

import { afterEach, describe, expect, it } from 'vitest'

import { CellSyntaxError, compileCell } from './compile'

interface Hooks {
  __bind?: (id: number, values: Record<string, unknown>) => void
  __step?: (id: number) => Promise<void>
  __stepSync?: (id: number) => void
}

const hooks = globalThis as unknown as Hooks & Record<string, unknown>

async function run(
  cell: CompiledCell,
  onStep: (id: number) => Promise<void> | void = () => {},
  onBind: (id: number, values: Record<string, unknown>) => void = () => {},
): Promise<unknown> {
  hooks.__bind = onBind
  hooks.__step = async id => onStep(id)
  hooks.__stepSync = (id) => {
    void onStep(id)
  }
  // Indirect eval runs in global scope, like the playground worker.
  // oxlint-disable-next-line no-eval -- executes compiled cells under test.
  ;(0, eval)(cell.prelude)
  // oxlint-disable-next-line no-eval -- see above.
  return await (0, eval)(cell.code)
}

afterEach(() => {
  for (const name of ['__bind', '__step', '__stepSync', 'total', 'apps', 'double', 'x'])
    delete hooks[name]
})

describe('compileCell', () => {
  it('fires hooks with editor line numbers, including loop bodies', async () => {
    const cell = compileCell([
      'const apps: string[] = [\'a\', \'bb\']',
      'let total = 0',
      'for (const name of apps) {',
      '  total += name.length',
      '}',
      'total',
    ].join('\n'))
    const lines: number[] = []

    const value = await run(cell, id => void lines.push(cell.steps[id]!.line))

    expect(value).toBe(3)
    expect(lines).toEqual([1, 2, 3, 4, 4, 6])
    expect(cell.steps.filter(step => step.top).map(step => step.line)).toEqual([1, 2, 3, 6])
  })

  it('keeps editor line numbers when compiling a single line with startLine', async () => {
    const cell = compileCell('const x = 41 + 1', { startLine: 7 })
    const lines: number[] = []

    await run(cell, id => void lines.push(cell.steps[id]!.line))

    expect(lines).toEqual([7])
    expect(hooks.x).toBe(42)
  })

  it('persists top-level declarations across cells', async () => {
    await run(compileCell('const total = 20\nfunction double(n: number) { return n * 2 }'))
    const value = await run(compileCell('double(total) + 2'))

    expect(value).toBe(42)
  })

  it('pauses while the async hook promise is pending', async () => {
    const cell = compileCell('let total = 1\ntotal += 1\ntotal')
    let release!: () => void
    const reached: number[] = []
    const pending = run(cell, (id) => {
      reached.push(cell.steps[id]!.line)
      if (cell.steps[id]!.line === 2)
        return new Promise<void>((resolve) => { release = resolve })
    })

    await new Promise(resolve => setTimeout(resolve, 10))
    expect(reached).toEqual([1, 2])
    expect(hooks.total).toBe(1)

    release()
    await expect(pending).resolves.toBe(2)
  })

  it('records sync function bodies without awaiting', async () => {
    const cell = compileCell('function double(n: number) {\n  return n * 2\n}\ndouble(2)')
    const steps: Array<{ async: boolean, line: number }> = []

    const value = await run(cell, id => void steps.push(cell.steps[id]!))

    expect(value).toBe(4)
    expect(steps).toContainEqual(expect.objectContaining({ async: false, line: 2 }))
  })

  it('reports declaration values and per-iteration loop bindings', async () => {
    const cell = compileCell([
      'const apps = [\'a\', \'bb\']',
      'for (let i = 0; i < 2; i++) {',
      '  const name = apps[i]!',
      '}',
      'for (const app of apps) total = app',
      'let total',
    ].join('\n'))
    const seen: Array<{ line: number, values: Record<string, unknown> }> = []

    await run(cell, () => {}, (id, values) => {
      seen.push({ line: cell.binds[id]!.line, values: { ...values } })
    })

    expect(seen).toEqual([
      { line: 1, values: { apps: ['a', 'bb'] } },
      { line: 2, values: { i: 0 } },
      { line: 3, values: { name: 'a' } },
      { line: 2, values: { i: 1 } },
      { line: 3, values: { name: 'bb' } },
      { line: 5, values: { app: 'a' } },
      { line: 5, values: { app: 'bb' } },
      { line: 6, values: { total: undefined } },
    ])
  })

  it('rejects TypeScript syntax that needs emit', () => {
    expect(() => compileCell('enum Mode { A }')).toThrow(CellSyntaxError)
  })

  it('reports parse errors with editor locations', () => {
    try {
      compileCell('const = 1', { startLine: 3 })
      expect.unreachable()
    }
    catch (error) {
      expect(error).toBeInstanceOf(CellSyntaxError)
      expect((error as CellSyntaxError).line).toBe(3)
    }
  })
})
