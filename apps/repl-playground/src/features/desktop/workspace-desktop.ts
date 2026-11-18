import type { Backend } from '../../backend/types'
import type { Workspace } from '../../store'

import { decodeThumbHash } from '../../preview'
import { decodeBitmap } from '../../runtime/rpc-resources'

export function createWorkspaceDesktop({ actions, store }: Workspace) {
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
  function setLive(live: boolean, intervalMs = 900): void {
    store.setState({ live })
    window.clearTimeout(liveTimer)
    if (!live)
      return
    const tick = async () => {
      const { backend, displays, live: stillLive } = store.getState()
      if (!stillLive || !backend)
        return
      for (const display of displays) {
        try {
          await captureLiveFrame(backend, display.id)
        }
        catch (error) {
          console.warn(`Live capture failed for display ${display.id}`, error)
        }
      }
      if (store.getState().live)
        liveTimer = window.setTimeout(() => void tick(), intervalMs)
    }
    void tick()
  }

  /**
   * Switches the backend scripts and the canvas use, and refreshes displays.
   * The device registry (`features/device`) owns backend lifecycles, so the
   * previous backend stays open.
   */
  async function activateBackend(backend: Backend | null): Promise<void> {
    setLive(false)
    store.setState({ axTree: null, backend, displays: [], liveFrames: {} })
    if (!backend)
      return
    const displays = await backend.listDisplays()
    if (store.getState().backend !== backend)
      return
    store.setState({ displays })
    // One snapshot per display so the canvas is not empty before live mode.
    for (const display of displays) {
      void captureLiveFrame(backend, display.id)
        .catch(error => console.warn(`Initial capture failed for display ${display.id}`, error))
    }
    void refreshAxTree().catch(() => {})
  }

  /**
   * Captures a display for the canvas. A display with no pixels on screen yet
   * shows the capture's ThumbHash preview first, so a new connection paints at
   * once; later captures replace the previous pixels when they load.
   */
  async function captureLiveFrame(backend: Backend, displayId: string): Promise<void> {
    const frame = await backend.captureDisplay(displayId, { logical: true })
    if (store.getState().backend !== backend)
      return
    const capturedAt = Date.now()
    const shown = store.getState().liveFrames[displayId]
    const preview = shown?.bitmap ? undefined : await decodeThumbHash(frame.thumbhash)
    if (store.getState().backend !== backend) {
      preview?.close()
      return
    }
    if (preview)
      actions.setLiveFrame(displayId, { bounds: frame.bounds, capturedAt, preview })
    const bitmap = await decodeBitmap(backend, frame)
    if (store.getState().backend !== backend) {
      bitmap?.close()
      return
    }
    actions.setLiveFrame(displayId, { bitmap, bounds: frame.bounds, capturedAt, preview, revealedAt: preview ? performance.now() : undefined })
  }

  return { activateBackend, refreshAxTree, setLive }
}
