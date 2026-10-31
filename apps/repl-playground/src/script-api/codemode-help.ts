import type { SdkDocumentation, SdkEntry, SdkType } from '@auv-js/sdk/documentation'

import { codeModeLimits } from '../runtime/codemode-limits'

export const codeModeRules = [
  'Call execute sequentially, including help queries. Wait for each tool result before starting the next call. Calls share one mutable desktop.',
  'Help describes the SDK contract, not verified support on every device. Platform calls can return UNIMPLEMENTED.',
  'Arguments must be data; SDK bigint IDs retain precision. Callbacks, AbortSignal, generators, and async iterators cannot cross the VM. The host cancels the whole execution.',
  'Local callbacks such as Array.map work inside the VM. Output and stored values must contain data, not functions. For WindowClient output, select its id or window metadata instead of the whole client.',
  'A plain { x, y } is window-local on WindowClient and screen space on auv.input. A text match clicks at the center of its screen bounds.',
  `Captures contain references. Use await image(capture) for PNG/JPEG output, bounded to ${codeModeLimits.maxImageDimension} by ${codeModeLimits.maxImageDimension}. Do not fetch or store full pixels.`,
  'Each call has fresh variables. store/load retains small JSON values only after success. Save window IDs and rebind with auv.windows.from(id).',
  'Await device calls. Failed or canceled scripts do not undo delivered input. Successful execution does not prove the requested application state.',
]

/** Search and describe SDK JSON metadata generated outside the browser; neither method executes a call. */
export function createApiHelp(catalog: SdkDocumentation) {
  const entries = [...catalog.entries].sort((a, b) => a.path.localeCompare(b.path))
  const summary = (entry: SdkEntry) => ({ description: entry.description, path: entry.path, signature: entry.signature })

  function search({ limit = 5, namespace, offset = 0, query = '' }: { limit?: number, namespace?: string, offset?: number, query?: string } = {}) {
    if (typeof query !== 'string' || (namespace !== undefined && typeof namespace !== 'string'))
      throw new TypeError('api.search expects query and namespace strings')
    if (!Number.isInteger(limit) || limit < 1 || limit > 10 || !Number.isInteger(offset) || offset < 0)
      throw new RangeError('api.search requires limit from 1 to 10 and a nonnegative offset')
    const terms = query.toLowerCase().split(/[^\p{L}\p{N}]+/u).filter(Boolean)
    const matches = entries.filter(entry => (!namespace || entry.path === namespace || entry.path.startsWith(`${namespace}.`)))
      .map(entry => ({ entry, score: terms.reduce((score, term) => score + (entry.path.toLowerCase().includes(term) ? 4 : 0) + (entry.description.toLowerCase().includes(term) ? 1 : 0), 0) }))
      .filter(item => terms.length === 0 || item.score > 0)
      .sort((a, b) => b.score - a.score || a.entry.path.localeCompare(b.entry.path))
    const next = offset + limit
    return { items: matches.slice(offset, next).map(item => summary(item.entry)), next: next < matches.length ? { offset: next } : null, remaining: Math.max(0, matches.length - next), total: matches.length }
  }

  function describe({ path }: { path: string }) {
    if (typeof path !== 'string' || !path)
      throw new TypeError('api.describe requires a nonempty path from api.search or a related type name')
    const match = entries.find(entry => entry.path === path) ?? (Object.hasOwn(catalog.types, path) ? catalog.types[path] : undefined)
    if (!match)
      return { found: false, message: `Unknown API or type: ${path}. Use api.search({ query: ... }) for exact paths.`, path }
    const result = { ...match, found: true, path }
    const types: Record<string, SdkType> = {}
    const omittedTypes: string[] = []
    let size = JSON.stringify(result).length
    for (const name of result.relatedTypes) {
      const definition = catalog.types[name]
      if (!definition)
        continue
      const cost = name.length + JSON.stringify(definition).length
      // Leave room for JSON framing within codemode's text output limit.
      if (size + cost > 12000) {
        omittedTypes.push(name)
      }
      else {
        types[name] = definition
        size += cost
      }
    }
    return { ...result, omittedTypes, types }
  }

  function help() {
    return {
      discovery: [
        'api.search({ query?, namespace?, limit?: 1..10, offset? }) returns SDK paths, signatures, and pagination, including selected constants.',
        'api.describe({ path }) returns full help, examples, and related types. Pass a related type name to inspect it.',
        'WindowClient is returned by auv.windows.resolve/get/list/from. Call its methods on that object, not on a global WindowClient.',
      ],
      examples: ['text(api.search({ query: "window text" }))', 'text(api.describe({ path: "WindowClient.findText" }))', 'text((await auv.windows.list()).map(win => ({ id: win.id, window: win.window })))'],
      limits: { ...codeModeLimits },
      namespaces: [...new Set(entries.map(entry => entry.path.split('.')[0]))],
      rules: codeModeRules,
      starter: ['auv.windows.list', 'auv.windows.resolve', 'WindowClient.findText', 'WindowClient.click', 'WindowClient.capture', 'image'].flatMap((path) => {
        const entry = entries.find(entry => entry.path === path)
        return entry ? [summary(entry)] : []
      }),
    }
  }

  return { describe, help, search }
}
