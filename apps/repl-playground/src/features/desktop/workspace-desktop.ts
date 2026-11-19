import type { Backend } from '../../backend/types'
import type { Workspace } from '../../store'

import { decodeThumbHash } from '../../preview'
import { decodeBitmap } from '../../runtime/rpc-resources'
import { useDeviceTarget } from '../device/device-target'

export function createWorkspaceDesktop({ actions, store }: Workspace) {
  let capture: AbortController | undefined
  let liveTimer: number | undefined

  /** Reloads the accessibility tree from the active backend, when supported. */
  async function refreshAxTree(): Promise<void> {
    const backend = store.getState().backend
    if (!backend?.accessibilityTree) {
      store.setState({ axTree: null })
      return
    }
    const axTree = await backend.accessibilityTree()
    if (store.getState().backend === backend)
      store.setState({ axTree })
  }

  /** Polls display captures into the canvas while live mode is on. */
  function setLive(live: boolean): void {
    store.setState({ live })
    if (live)
      startCapture(store.getState().backend)
    else
      stopCapture()
  }

  function watchDevice(): () => void {
    const activate = (backend: Backend | null) => {
      if (backend !== store.getState().backend)
        store.setState({ axTree: null, backend, displays: [], live: false, liveFrames: {} })
      startCapture(backend)
      void refreshAxTree().catch(() => {})
    }
    const unsubscribe = useDeviceTarget.subscribe(({ backend }, previous) => {
      if (backend !== previous.backend)
        activate(backend)
    })
    activate(useDeviceTarget.getState().backend)
    return () => {
      unsubscribe()
      stopCapture()
    }
  }

  function startCapture(backend: Backend | null): void {
    stopCapture()
    if (!backend)
      return
    capture = new AbortController()
    const { signal } = capture
    void captureDisplays(backend, signal).catch((error) => {
      if (!signal.aborted)
        console.warn('Loading the desktop failed', error)
    })
  }

  function stopCapture(): void {
    capture?.abort()
    capture = undefined
    window.clearTimeout(liveTimer)
  }

  async function captureDisplays(backend: Backend, signal: AbortSignal): Promise<void> {
    const displays = await backend.listDisplays()
    if (signal.aborted)
      return
    store.setState({ displays })
    const tick = async () => {
      for (const display of displays) {
        if (signal.aborted)
          return
        try {
          await captureLiveFrame(backend, display.id, signal)
        }
        catch (error) {
          if (!signal.aborted)
            console.warn(`Capture failed for display ${display.id}`, error)
        }
      }
      if (!signal.aborted && store.getState().live)
        liveTimer = window.setTimeout(() => void tick(), 900)
    }
    await tick()
  }

  /**
   * Captures a display for the canvas. A display with no pixels on screen yet
   * shows the capture's ThumbHash preview first, so a new connection paints at
   * once; later captures replace the previous pixels when they load.
   */
  async function captureLiveFrame(backend: Backend, displayId: string, signal: AbortSignal): Promise<void> {
    const frame = await backend.captureDisplay(displayId, { logical: true, signal })
    if (signal.aborted)
      return
    const capturedAt = Date.now()
    const shown = store.getState().liveFrames[displayId]
    const preview = shown?.bitmap ? undefined : await decodeThumbHash(frame.thumbhash)
    if (signal.aborted) {
      preview?.close()
      return
    }
    if (preview)
      actions.setLiveFrame(displayId, { bounds: frame.bounds, capturedAt, preview })
    const bitmap = await decodeBitmap(backend, frame)
    if (signal.aborted) {
      bitmap.close()
      return
    }
    actions.setLiveFrame(displayId, { bitmap, bounds: frame.bounds, capturedAt, preview, revealedAt: preview ? performance.now() : undefined })
  }

  return { refreshAxTree, setLive, watchDevice }
}
