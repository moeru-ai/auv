import type { RunnerClient } from '@auv-js/sdk'
import type { QuickJSHandle } from 'quickjs-emscripten-core'

import type { RunOutcome, RunRequest } from '../runtime/protocol'

import variant from '@jitl/quickjs-singlefile-browser-release-sync'
import catalog from 'virtual:auv-codemode-help'

import { newQuickJSWASMModuleFromVariant } from 'quickjs-emscripten-core'

import { codeModeLimits } from '../runtime/codemode-limits'
import { createApiHelp } from '../script-api/codemode-help'
import { toWire } from './wire-value'

const apiHelp = createApiHelp(catalog)

// NOTICE(codemode-wasm): the browser single-file variant embeds WASM, so
// Vite workers and Node tests do not fetch a package-relative asset.
// See quickjs-emscripten README "WebAssembly loading".
const quickJS = newQuickJSWASMModuleFromVariant(variant)

/**
 * An isolated VM per tool call. Only supplied SDK functions cross into the
 * host; guest code has no browser, network, filesystem or worker globals.
 */
export async function runCodeMode(request: RunRequest, globals: Record<string, unknown>, signal: AbortSignal, onOutput: (item: NonNullable<RunOutcome['codemode']>['content'][number]) => void = () => {}): Promise<RunOutcome> {
  const vm = (await quickJS).newContext()
  const deadline = Date.now() + request.codemode!.timeoutMs
  vm.runtime.setMemoryLimit(codeModeLimits.memoryMiB * 1024 * 1024)
  vm.runtime.setMaxStackSize(512 * 1024)
  vm.runtime.setInterruptHandler(() => signal.aborted || Date.now() >= deadline)
  const content: NonNullable<RunOutcome['codemode']>['content'] = []
  let store = structuredClone(request.codemode!.store)
  let outputChars = 0
  let imageChars = 0
  let closed = false
  let failure: ((error: Error) => void) | undefined
  const deferreds = new Set<ReturnType<typeof vm.newPromise>>()
  const bindWindow = (globals.auv as Partial<RunnerClient> | undefined)?.windows?.from
  // NOTICE(codemode-callbacks): callbacks/iterators need explicit VM
  // marshalling. Reject them until an approved streaming/callback slice.
  const validate = vm.unwrapResult(vm.evalCode(`value => JSON.stringify(value, (key, item) => {
    if (typeof item === 'function') throw new TypeError('Functions cannot cross the codemode VM boundary. Pass data only; for WindowClient output, select window.id or window.window.');
    if (typeof item === 'bigint') return String(item);
    return item;
  })`))
  const isArray = vm.unwrapResult(vm.evalCode('Array.isArray'))
  const fromVM = (handle: QuickJSHandle): unknown => {
    const checked = vm.unwrapResult(vm.callFunction(validate, vm.undefined, handle))
    checked.dispose()
    const type = vm.typeof(handle)
    if (type !== 'object' || vm.eq(handle, vm.null))
      return vm.dump(handle)
    const array = vm.unwrapResult(vm.callFunction(isArray, vm.undefined, handle)).consume(result => vm.eq(result, vm.true))
    const value: Record<string, unknown> | unknown[] = array ? [] : {}
    const keys = vm.unwrapResult(vm.getOwnPropertyNames(handle, { numbersAsStrings: true, onlyEnumerable: true, strings: true }))
    try {
      for (const key of keys) {
        const name = vm.getString(key)
        const child = vm.getProp(handle, key)
        try {
          Object.defineProperty(value, name, { configurable: true, enumerable: true, value: fromVM(child), writable: true })
        }
        finally { child.dispose() }
      }
      return value
    }
    finally { keys.dispose() }
  }
  const pump = () => {
    if (closed)
      return
    const jobs = vm.runtime.executePendingJobs()
    if (jobs.error) {
      const error = vm.dump(jobs.error) as { message?: string }
      jobs.error.dispose()
      failure?.(new Error(error.message ?? 'Code execution failed'))
    }
  }
  const toVM = (value: unknown, asyncFunctions = true, depth = 0, validateArguments = true): QuickJSHandle => {
    if (depth > 20)
      throw new Error('SDK value is too deeply nested')
    if (value === undefined)
      return vm.undefined
    if (value === null)
      return vm.null
    if (typeof value === 'string')
      return vm.newString(value)
    if (typeof value === 'number')
      return vm.newNumber(value)
    if (typeof value === 'boolean')
      return value ? vm.true : vm.false
    if (typeof value === 'bigint')
      return vm.newBigInt(value)
    if (typeof value === 'function') {
      return vm.newFunction(value.name, (...handles) => {
        if (signal.aborted || closed)
          throw new Error('Codemode canceled')
        const args = handles.map(handle => validateArguments ? fromVM(handle) : vm.dump(handle))
        // Binding an ID is a synchronous SDK operation. The returned
        // WindowClient still carries asynchronous device methods.
        if (!asyncFunctions || value === bindWindow)
          return toVM(value(...args), asyncFunctions)
        const pending = vm.newPromise()
        deferreds.add(pending)
        void Promise.resolve().then(() => {
          if (closed || signal.aborted)
            throw new Error('Codemode canceled')
          return value(...args)
        }).then((result) => {
          if (closed)
            return
          const handle = toVM(result)
          pending.resolve(handle)
          handle.dispose()
        }).catch((error: unknown) => {
          if (closed)
            return
          const handle = error instanceof Error ? vm.newError(error) : vm.newError(String(error))
          if (error instanceof Error && 'rpcCode' in error && typeof error.rpcCode === 'number') {
            const code = vm.newNumber(error.rpcCode)
            vm.setProp(handle, 'rpcCode', code)
            code.dispose()
          }
          pending.reject(handle)
          handle.dispose()
        }).finally(() => {
          if (!closed) {
            pending.dispose()
            deferreds.delete(pending)
            pump()
          }
        })
        return pending.handle
      })
    }
    if (typeof value !== 'object')
      throw new Error('Unsupported SDK value')
    if (Symbol.asyncIterator in value)
      throw new Error('SDK async iterators are not supported in codemode yet')
    if (ArrayBuffer.isView(value) && value.byteLength > 4096)
      throw new Error('Use image(capture) for bounded capture pixels')
    const handle = Array.isArray(value) || ArrayBuffer.isView(value) ? vm.newArray() : vm.newObject()
    try {
      for (const [key, item] of Object.entries(value)) {
        const child = toVM(item, asyncFunctions, depth + 1)
        vm.setProp(handle, key, child)
        child.dispose()
      }
      return handle
    }
    catch (error) {
      handle.dispose()
      throw error
    }
  }
  const install = (name: string, value: unknown, asyncFunctions = true) => {
    const handle = toVM(value, asyncFunctions, 0, name !== '__bind')
    vm.setProp(vm.global, name, handle)
    handle.dispose()
  }
  const text = (value: unknown) => {
    const output = typeof value === 'string' ? value : JSON.stringify(value, (_key, item) => typeof item === 'bigint' ? String(item) : item) ?? 'undefined'
    if (content.length >= 100 || outputChars + output.length > codeModeLimits.maxTextCharacters)
      throw new Error(`Codemode output exceeds ${codeModeLimits.maxTextCharacters} characters or 100 items; filter results before output`)
    outputChars += output.length
    const item = { text: output, type: 'text' as const }
    content.push(item)
    onOutput(item)
  }
  const abort = () => failure?.(new Error('Codemode canceled. Delivered inputs are not undone.'))
  const timer = setTimeout(() => failure?.(new Error('Codemode timed out. Delivered inputs are not undone.')), request.codemode!.timeoutMs)
  signal.addEventListener('abort', abort, { once: true })
  try {
    for (const [name, value] of Object.entries(globals)) install(name, value, name === 'auv' || name === 'image' || name === '__step')
    // Inspection must summarize inside the VM, before a value crosses into
    // the host. Keep this shared serializer self-contained for both realms.
    vm.unwrapResult(vm.evalCode(`{
      const inspect = (${toWire.toString()});
      const bind = globalThis.__bind;
      globalThis.__bind = (id, values) => bind(id, inspect(values));
    }`)).dispose()
    install('api', apiHelp, false)
    // State lives in host JSON, so it cannot retain SDK function closures,
    // image bytes, or mutable references from an earlier VM.
    install('store', (key: string, value: unknown) => {
      if (typeof key !== 'string')
        throw new Error('store key must be a string')
      const next = { ...store }
      if (value === undefined)
        delete next[key]
      else Object.defineProperty(next, key, { configurable: true, enumerable: true, value: structuredClone(value), writable: true })
      if (JSON.stringify(next).length > codeModeLimits.maxStoreCharacters)
        throw new Error(`Codemode store exceeds ${codeModeLimits.maxStoreCharacters} characters`)
      store = next
    }, false)
    install('load', (key: string) => Object.hasOwn(store, key) ? structuredClone(store[key]) : undefined, false)
    install('text', text, false)
    install('show', text, false)
    install('console', { error: text, info: text, log: text, warn: text }, false)
    // The host resolves pixels only for this explicit request. They never
    // enter VM memory or the persistent store.
    install('image', async (capture: unknown, options?: unknown) => {
      const image = await (globals.image as (capture: unknown, options?: unknown) => Promise<{ data: string, mimeType: string }>)(capture, options)
      // NOTICE(codemode-frame-budget): Exterior forwards ACP frames with a
      // 1,048,576-character payload limit. Leave room for text and JSON framing.
      if (content.length >= 100 || image.data.length > 512000 || imageChars + image.data.length > 750000 || content.filter(item => item.type === 'image').length >= 4)
        throw new Error('Codemode image output exceeds its limit')
      imageChars += image.data.length
      const item = { ...image, type: 'image' as const }
      content.push(item)
      onOutput(item)
    })
    vm.unwrapResult(vm.evalCode(request.prelude)).dispose()
    const evaluated = vm.unwrapResult(vm.evalCode(request.code))
    try {
      const result = await new Promise<Awaited<ReturnType<typeof vm.resolvePromise>>>((resolve, reject) => {
        failure = reject
        if (signal.aborted) {
          abort()
          return
        }
        void vm.resolvePromise(evaluated).then(resolve, reject)
        pump()
      })
      const value = vm.unwrapResult(result)
      try {
        const returned = fromVM(value)
        if (returned !== undefined)
          text(returned)
        return { codemode: { content, store }, status: 'ok', value: undefined }
      }
      finally { value.dispose() }
    }
    finally { evaluated.dispose() }
  }
  catch (error) {
    const message = signal.aborted ? 'Codemode canceled. Delivered inputs are not undone.' : error instanceof Error ? error.message : String(error)
    return { codemode: { content, store: request.codemode!.store }, message, status: 'error' }
  }
  finally {
    closed = true
    clearTimeout(timer)
    signal.removeEventListener('abort', abort)
    for (const pending of deferreds) pending.dispose()
    validate.dispose()
    isArray.dispose()
    vm.dispose()
  }
}
