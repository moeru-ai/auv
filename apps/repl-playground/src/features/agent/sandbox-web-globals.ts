import { TextDecoder, TextEncoder } from '@exodus/bytes/encoding-lite.js'
import { Headers } from 'headers-polyfill'

Object.assign(globalThis, { Headers, TextDecoder, TextEncoder })
