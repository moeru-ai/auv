import { TextDecoder, TextEncoder } from '@exodus/bytes/encoding-lite.js'
import { Headers } from 'headers-polyfill'

declare function runnerWait(milliseconds: number): Promise<void>

const timers = new Set<number>()
let nextTimer = 0
Object.assign(globalThis, {
  clearTimeout: (id: number) => timers.delete(id),
  Headers,
  setTimeout(callback: () => void, milliseconds = 0) {
    const id = ++nextTimer
    timers.add(id)
    void runnerWait(milliseconds).then(() => {
      if (timers.delete(id))
        callback()
    }, () => timers.delete(id))
    return id
  },
  TextDecoder,
  TextEncoder,
})
