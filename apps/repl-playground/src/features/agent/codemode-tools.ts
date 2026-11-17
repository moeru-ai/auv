import type { RectLike, RunnerClient, WindowClient } from '@auv-js/sdk'
import type { CodemodeTool } from '@exterioragentprotocol/client-sdk/codemode'

import { ImageEncoding, Position } from '@auv-js/sdk'
import { encodeBase64 } from '@moeru/std'
import { z } from 'zod'

type Capture = Awaited<ReturnType<WindowClient['capture']>>['capture']
type InputAction = Awaited<ReturnType<WindowClient['typeText']>>
type Tool = Omit<CodemodeTool, 'execute'> & {
  execute: (getRunner: () => RunnerClient, value: unknown, signal: AbortSignal) => Promise<unknown>
}

const nonblank = z.string().regex(/\S/, 'Expected nonblank text')
const imageLimit = 1024
const imageDimension = z.int().min(1).max(imageLimit)
const windowTarget = z.strictObject({ windowId: nonblank })
const position = z.object({ x: z.number(), y: z.number() })
const windowPoint = windowTarget.extend({ space: z.enum(['window', 'screen']).optional(), ...position.shape })
const size = z.object({ height: z.number(), width: z.number() })
const rect = position.extend(size.shape)
const windowResult = z.object({
  applicationId: z.string().optional(),
  appName: z.string().optional(),
  frame: rect.optional(),
  isMain: z.boolean(),
  isVisible: z.boolean(),
  processId: z.number().optional(),
  title: z.string().optional(),
  windowId: nonblank,
})
const captureResult = z.object({
  bounds: rect.optional(),
  captureId: nonblank,
  origin: position.extend({ coordinateSpace: z.object({
    case: z.string().optional(),
    value: z.union([z.string(), z.boolean()]).optional(),
  }) }).optional(),
  pixelSize: size.optional(),
  scaleFactor: z.number(),
})
const actionResult = z.object({
  attempts: z.array(z.object({ message: z.string().optional(), path: z.number(), succeeded: z.boolean() })),
  clipboardDisturbance: z.number(),
  focusDisturbance: z.number(),
  mouseDisturbance: z.number(),
  selectedPath: z.number(),
  verified: z.boolean(),
})

function defineTool<Input extends z.ZodType, Output extends z.ZodType>({ description, input, name, output, run }: {
  description: string
  input: Input
  name: string
  output: Output
  run: (runner: RunnerClient, args: z.output<Input>, signal: AbortSignal) => Promise<z.output<Output>>
}): Tool {
  return {
    description,
    async execute(getRunner, value, signal) {
      signal.throwIfAborted()
      const args = input.parse(value === undefined ? {} : value)
      return run(getRunner(), args, signal)
    },
    inputSchema: z.toJSONSchema(input, { io: 'input' }),
    name,
    outputSchema: z.toJSONSchema(output),
  }
}

const tools = [
  defineTool({
    description: 'List windows on the selected Device with IDs and logical screen-space frames.',
    input: z.strictObject({}),
    name: 'list_windows',
    output: z.array(windowResult),
    async run(runner, _args, signal) {
      return (await runner.windows.list({ signal })).map(windowMetadata)
    },
  }),
  defineTool({
    description: 'Resolve a window by app or title. Use at most one of appName, applicationId, pid, and frontmost, and at most one of title and titleContains.',
    input: z.strictObject({
      applicationId: nonblank.optional(),
      appName: nonblank.optional(),
      frontmost: z.boolean().optional(),
      pid: z.uint32().min(1).optional(),
      title: z.string().optional(),
      titleContains: nonblank.optional(),
    }),
    name: 'resolve_window',
    output: windowResult,
    async run(runner, selector, signal) {
      return windowMetadata(await runner.windows.resolve(selector, { signal }))
    },
  }),
  defineTool({
    description: 'Refresh window metadata by an ID from list_windows or resolve_window.',
    input: windowTarget,
    name: 'get_window',
    output: windowResult,
    async run(runner, { windowId }, signal) {
      return windowMetadata(await runner.windows.get(windowId, { signal }))
    },
  }),
  defineTool({
    description: 'Find text in one window with OCR. Matches have logical screen-space bounds. The capture contains metadata and a reference.',
    input: windowTarget.extend({ query: z.string() }),
    name: 'find_window_text',
    output: z.object({
      capture: captureResult.optional(),
      matches: z.array(z.object({ bounds: rect.optional(), confidence: z.number(), text: z.string() })),
    }),
    async run(runner, { query, windowId }, signal) {
      const { capture, matches } = await runner.windows.from(windowId).findText(query, { signal })
      return {
        capture: capture && captureMetadata(capture),
        matches: matches.map(({ bounds, confidence, text }) => ({ bounds: bounds && rectangle(bounds), confidence, text })),
      }
    },
  }),
  defineTool({
    description: 'Deliver a click to this window at a logical point. The default is window-local. Use space: "screen" for OCR match bounds. Verify the effect with a separate read.',
    input: windowPoint,
    name: 'click_window',
    output: z.object({ action: actionResult.optional(), screenPoint: position.optional(), windowPoint: position.optional() }),
    async run(runner, args, signal) {
      const { action, screenPoint, windowPoint } = await runner.windows.from(args.windowId).click(windowPosition(args), undefined, { signal })
      return {
        action: action && actionMetadata(action),
        screenPoint: screenPoint && { x: screenPoint.x, y: screenPoint.y },
        windowPoint: windowPoint && { x: windowPoint.x, y: windowPoint.y },
      }
    },
  }),
  defineTool({
    description: 'Capture a window. Returns captureId and metadata. Use get_capture_image to fetch bounded pixels.',
    input: windowTarget,
    name: 'capture_window',
    output: captureResult,
    async run(runner, { windowId }, signal) {
      const { capture } = await runner.windows.from(windowId).capture({ signal })
      return captureMetadata(capture)
    },
  }),
  defineTool({
    description: `Fetch a capture as a PNG or JPEG image block. Pass the block to image() to show it. Each dimension is capped at ${imageLimit} pixels.`,
    input: z.strictObject({
      captureId: nonblank,
      encoding: z.enum(['png', 'jpeg']).optional(),
      maxSize: z.strictObject({ height: imageDimension, width: imageDimension }).optional(),
    }),
    name: 'get_capture_image',
    output: z.object({ data: z.string(), mimeType: z.enum(['image/png', 'image/jpeg']), type: z.literal('image') }),
    async run(runner, { captureId, encoding, maxSize = { height: imageLimit, width: imageLimit } }, signal) {
      const pixels = await runner.captures.image(captureId, { encoding: encoding === 'jpeg' ? ImageEncoding.JPEG : ImageEncoding.PNG, maxSize, signal })
      signal.throwIfAborted()
      return { data: encodeBase64(pixels.data), mimeType: encoding === 'jpeg' ? 'image/jpeg' : 'image/png', type: 'image' } as const
    },
  }),
  defineTool({
    description: 'Type text into the focused control of the selected window. Returns input delivery evidence.',
    input: windowTarget.extend({ text: z.string() }),
    name: 'type_window_text',
    output: actionResult,
    async run(runner, { text, windowId }, signal) {
      return actionMetadata(await runner.windows.from(windowId).typeText(text, undefined, { signal }))
    },
  }),
  defineTool({
    description: 'Press one key combination in a window. Use a string such as "cmd+a" or an array of keys. Returns input delivery evidence.',
    input: windowTarget.extend({ keys: z.union([nonblank, z.array(nonblank).nonempty()]) }),
    name: 'press_window_keys',
    output: actionResult,
    async run(runner, { keys, windowId }, signal) {
      return actionMetadata(await runner.windows.from(windowId).pressKeys(keys, undefined, { signal }))
    },
  }),
  defineTool({
    description: 'Wheel-scroll a window at a logical point. Positive deltaY scrolls down. Use window-local coordinates or space: "screen".',
    input: windowPoint.extend({ deltaX: z.number().optional(), deltaY: z.number() }),
    name: 'scroll_window',
    output: z.object({ action: actionResult.optional() }),
    async run(runner, args, signal) {
      const { action } = await runner.windows.from(args.windowId).scroll(windowPosition(args), { deltaX: args.deltaX, deltaY: args.deltaY }, undefined, { signal })
      return { action: action && actionMetadata(action) }
    },
  }),
]

export const codeModeToolDescriptions: Omit<CodemodeTool, 'execute'>[] = tools.map(({ description, inputSchema, name, outputSchema }) => ({
  description,
  inputSchema,
  name,
  outputSchema,
}))

export function createCodeModeTools(getRunner: () => RunnerClient): CodemodeTool[] {
  return tools.map(({ execute, ...tool }) => ({
    ...tool,
    execute: (value, { signal }) => execute(getRunner, value, signal),
  }))
}

function actionMetadata({ attempts, clipboardDisturbance, focusDisturbance, mouseDisturbance, selectedPath, verified }: InputAction) {
  return { attempts: attempts.map(({ message, path, succeeded }) => ({ message, path, succeeded })), clipboardDisturbance, focusDisturbance, mouseDisturbance, selectedPath, verified }
}

function captureMetadata(capture: Capture) {
  const captureId = capture?.ref?.captureId
  if (!capture || !captureId)
    throw new Error('The Runner did not return a capture reference')
  const { bounds, origin, pixelSize, scaleFactor } = capture
  return {
    bounds: bounds && rectangle(bounds),
    captureId,
    origin: origin && { coordinateSpace: origin.coordinateSpace, x: origin.x, y: origin.y },
    pixelSize: pixelSize && { height: pixelSize.height, width: pixelSize.width },
    scaleFactor,
  }
}

function rectangle({ height, width, x, y }: RectLike) {
  return { height, width, x, y }
}

function windowMetadata({ id: windowId, window }: WindowClient) {
  const { applicationId, applicationName, frame, isMain, isVisible, processId, title } = window
  return { applicationId, appName: applicationName, frame: frame && rectangle(frame), isMain, isVisible, processId, title, windowId }
}

function windowPosition({ space, x, y }: z.output<typeof windowPoint>) {
  return space === 'screen' ? Position.screen(x, y) : { x, y }
}
