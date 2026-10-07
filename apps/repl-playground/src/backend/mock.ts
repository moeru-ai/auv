import type { ClickOptions, Point, Rect, ScrollDelta, ScrollObservation, TextMatch, WindowSelector } from '../script-api/api'
import type { AxNode, Backend, CapturedFrame, DisplayInfo, InputReceipt, ScrollUntilOutcome, ScrollUntilRequest, TextSearchResult, WindowInfo } from './types'

interface MockWindow extends WindowInfo {
  background: string
  /** Window-specific painting under the widgets (e.g. a player bar or scrollbar). */
  decorate?: (ctx: OffscreenCanvasRenderingContext2D, frame: Rect) => void
  widgets: () => Widget[]
}

interface Widget {
  action?: 'done' | 'focus' | 'increment' | 'play'
  /** Window-relative viewport the widget scrolls in; outside it, it is neither painted, hit nor recognized. */
  clip?: Rect
  id: string
  label: string
  /** Window-relative rectangle. */
  rect: Rect
  role: 'button' | 'input' | 'text'
  rowIndex?: number
  songIndex?: number
}

// Window-relative scroll viewport of the Music song list, and one row's height.
const MUSIC_VIEW: Rect = { height: 420, width: 448, x: 16, y: 82 }
const SONG_ROW = 48

/** A deterministic playlist long enough to need several screens of scrolling. */
const SONGS: Array<{ artist: string, title: string }> = [
  ['Morning Static', 'Lumen Drive'],
  ['Paper Satellites', 'Hollow Pines'],
  ['Glass Harbor', 'Mira Vale'],
  ['Night Bus Home', 'The Quiet Hours'],
  ['Cobalt Summer', 'Lumen Drive'],
  ['Small Fires', 'Ada North'],
  ['Undertow', 'Hollow Pines'],
  ['Signal Lost', 'Kite & Wire'],
  ['Parallel Lines', 'Mira Vale'],
  ['Slow Orbit', 'Ada North'],
  ['Weather Report', 'The Quiet Hours'],
  ['Afterglow', 'Kite & Wire'],
  ['Northbound', 'Lumen Drive'],
  ['Hush', 'Ada North'],
  ['Tin Can Telephone', 'Hollow Pines'],
  ['Late Arrival', 'Mira Vale'],
  ['Static Bloom', 'Kite & Wire'],
  ['Lantern Festival', 'The Quiet Hours'],
  ['Open Water', 'Lumen Drive'],
  ['Copper Moon', 'Ada North'],
  ['Field Notes', 'Hollow Pines'],
  ['Vapor Trail', 'Kite & Wire'],
  ['Second Light', 'Mira Vale'],
  ['Low Tide', 'The Quiet Hours'],
  ['Silver Thread', 'Ada North'],
  ['Reply', 'Lumen Drive'],
  ['Overpass', 'Hollow Pines'],
  ['Long Exposure', 'Kite & Wire'],
  ['Departure Board', 'Mira Vale'],
  ['Echo Valley', 'The Quiet Hours'],
  ['Remember', 'Ada North'],
  ['Quiet Engines', 'Lumen Drive'],
  ['Starling', 'Hollow Pines'],
  ['Neon Rain', 'Kite & Wire'],
  ['Harbor Lights', 'Mira Vale'],
  ['Last Train', 'The Quiet Hours'],
  ['Morning Again', 'Ada North'],
  ['Distant Shore', 'Lumen Drive'],
  ['Closing Credits', 'Hollow Pines'],
  ['Encore', 'Kite & Wire'],
].map(([title, artist]) => ({ artist: artist!, title: title! }))

const DISPLAYS: DisplayInfo[] = [
  { frame: { height: 900, width: 1440, x: 0, y: 0 }, id: '1', name: 'Built-in Retina Display', primary: true, scale: 2 },
  { frame: { height: 800, width: 1280, x: 1440, y: 50 }, id: '2', name: 'External Display', primary: false, scale: 1 },
]

const FONT = '"Inter", "Helvetica Neue", Arial, sans-serif'

/** `rect` clipped to `to`; callers check the areas overlap. */
function clip(rect: Rect, to: Rect): Rect {
  const x = Math.max(rect.x, to.x)
  const y = Math.max(rect.y, to.y)
  return { height: Math.min(rect.y + rect.height, to.y + to.height) - y, width: Math.min(rect.x + rect.width, to.x + to.width) - x, x, y }
}

function includes(text: string, query: string): boolean {
  return text.toLowerCase().includes(query.toLowerCase())
}

/**
 * A deterministic in-browser desktop: two displays, a todo app, a counter and
 * a music app whose song list scrolls.
 * Clicks and keys mutate the scene so scripts observe real state changes;
 * OCR returns exact text geometry from the scene graph.
 */
export class MockBackend implements Backend {
  readonly kind = 'mock'
  readonly label = 'Mock desktop'
  #captureCount = 0
  /** Rendered captures by reference, newest last. */
  readonly #captures = new Map<string, OffscreenCanvas>()
  #count = 0
  #draft = ''
  #focused = false
  /** Music list scroll offset in logical pixels, clamped to the list. */
  #musicScroll = 0
  #nowPlaying: null | number = null
  readonly #todos = [
    { done: false, text: 'Ship the AUV playground' },
    { done: false, text: 'Reply to issue #42' },
    { done: true, text: 'Buy oat milk' },
    { done: false, text: 'Review device placement doc' },
  ]

  readonly #windows: MockWindow[] = [
    {
      app: 'Todos',
      background: '#fdfcf8',
      bundleId: 'dev.auv.mock.todos',
      frame: { height: 560, width: 760, x: 120, y: 120 },
      id: 'w-todos',
      pid: 4101,
      title: 'Todos — Inbox',
      widgets: () => this.#todoWidgets(),
    },
    {
      app: 'Counter',
      background: '#f4f7fb',
      bundleId: 'dev.auv.mock.counter',
      frame: { height: 300, width: 420, x: 1700, y: 220 },
      id: 'w-counter',
      pid: 4102,
      title: 'Counter',
      widgets: () => [
        { id: 'count', label: `Count: ${this.#count}`, rect: { height: 48, width: 360, x: 30, y: 70 }, role: 'text' },
        { action: 'increment', id: 'inc', label: 'Increment', rect: { height: 56, width: 200, x: 110, y: 170 }, role: 'button' },
      ],
    },
    {
      app: 'Music',
      background: '#fafafa',
      bundleId: 'dev.auv.mock.music',
      decorate: (ctx, frame) => this.#decorateMusic(ctx, frame),
      frame: { height: 580, width: 480, x: 920, y: 120 },
      id: 'w-music',
      pid: 4103,
      title: 'Music — Liked Songs',
      widgets: () => this.#musicWidgets(),
    },
  ]

  /** Scene graph as an accessibility tree, the shape a real AX snapshot would have. */
  async accessibilityTree(): Promise<AxNode | null> {
    await delay(20)
    const ROLE = { button: 'AXButton', input: 'AXTextField', text: 'AXStaticText' } as const
    return {
      children: this.#windows.map((window, w) => ({
        children: window.widgets().map((widget, k) => ({
          actions: widget.action ? ['AXPress'] : undefined,
          children: [],
          frame: offset(widget.rect, window.frame),
          label: widget.role === 'input' ? undefined : widget.label,
          path: `0/${w}/${k}`,
          role: ROLE[widget.role],
          value: widget.role === 'input' ? this.#draft : undefined,
        })),
        frame: { ...window.frame },
        label: window.title,
        path: `0/${w}`,
        role: 'AXWindow',
        value: window.app,
      })),
      label: 'Desktop',
      path: '0',
      role: 'AXApplicationGroup',
    }
  }

  async activateApp(bundleId: string): Promise<InputReceipt> {
    await delay(20)
    if (!this.#windows.some(window => window.bundleId === bundleId))
      throw new Error(`No running application with bundle ID ${bundleId}`)
    // NOTICE(mock-activation): mock windows never overlap, so activation has
    // nothing to reorder; it only validates the bundle ID like AUV would.
    return { path: 'verifiedForeground' }
  }

  async beginRun(): Promise<string | undefined> {
    return undefined
  }

  async captureDisplay(displayId?: string, options?: { logical?: boolean }): Promise<CapturedFrame> {
    const display = this.#display(displayId)
    await delay(40)
    return this.#render(display.frame, options?.logical ? 1 : display.scale, `display:${display.id}`)
  }

  async captureImage(frame: CapturedFrame, maxSize: { height: number, width: number }): Promise<Blob> {
    const source = this.#captures.get(frame.ref)
    if (!source)
      throw new Error(`capture ${frame.ref} was not found: it was evicted; capture again`)
    const fit = Math.min(1, maxSize.width / source.width, maxSize.height / source.height)
    const canvas = new OffscreenCanvas(Math.max(1, Math.round(source.width * fit)), Math.max(1, Math.round(source.height * fit)))
    canvas.getContext('2d')!.drawImage(source, 0, 0, canvas.width, canvas.height)
    return await canvas.convertToBlob({ type: 'image/png' })
  }

  async captureWindow(windowId: string): Promise<CapturedFrame> {
    const window = this.#window(windowId)
    const display = this.#displayAt(window.frame)
    await delay(30)
    return this.#render(window.frame, display.scale, `window:${window.id}`)
  }

  async clickScreen(point: Point): Promise<InputReceipt> {
    await delay(25)
    this.#hit(point)
    return { path: 'mock-pointer', point }
  }

  async clickWindow(windowId: string, point: Point, _options?: ClickOptions): Promise<InputReceipt> {
    const frame = this.#window(windowId).frame
    return this.clickScreen({ x: frame.x + point.x, y: frame.y + point.y })
  }

  async dispose(): Promise<void> {}

  async endRun(): Promise<void> {}

  async findDisplayText(query: string, displayId?: string, area?: Rect): Promise<TextSearchResult> {
    const capture = await this.captureDisplay(displayId)
    await delay(120)
    return { capture, matches: this.#text(capture.bounds, area).filter(match => includes(match.text, query)) }
  }

  async findWindowText(windowId: string, query: string, area?: Rect): Promise<TextSearchResult> {
    const capture = await this.captureWindow(windowId)
    await delay(90)
    return { capture, matches: this.#text(capture.bounds, area).filter(match => includes(match.text, query)) }
  }

  async listDisplays(): Promise<DisplayInfo[]> {
    return structuredClone(DISPLAYS)
  }

  async listWindows(): Promise<WindowInfo[]> {
    return this.#windows.map(toInfo)
  }

  async pressKey(key: string): Promise<InputReceipt> {
    await delay(15)
    if (this.#focused && /^(?:return|enter)$/i.test(key) && this.#draft.trim()) {
      this.#todos.push({ done: false, text: this.#draft.trim() })
      this.#draft = ''
    }
    return { path: 'mock-keyboard' }
  }

  async recognizeText(frame: CapturedFrame, area?: Rect): Promise<TextSearchResult> {
    await delay(100)
    const matches = this.#text(frame.bounds, area)
    return { matches, text: matches.map(match => match.text).join('\n') }
  }

  async resolveWindow(selector: WindowSelector): Promise<WindowInfo> {
    await delay(10)
    const window = this.#windows.find(candidate =>
      (!selector.bundleId || candidate.bundleId === selector.bundleId)
      && (!selector.appName || candidate.app === selector.appName)
      && (!selector.pid || candidate.pid === selector.pid)
      && (!selector.title || candidate.title === selector.title)
      && (!selector.titleContains || candidate.title?.includes(selector.titleContains)))
    if (!window)
      throw new Error(`No mock window matches ${JSON.stringify(selector)}`)
    return toInfo(window)
  }

  async scrollWindow(windowId: string, point: Point, delta: ScrollDelta): Promise<InputReceipt> {
    const frame = this.#window(windowId).frame
    await delay(20)
    this.#scroll(windowId, point, delta)
    return { path: 'mock-wheel', point: { x: frame.x + point.x, y: frame.y + point.y } }
  }

  /**
   * Same stop order as the AUV Runner: the built-in condition, then the end
   * and the budget, then the client predicate. Only the Music song list
   * scrolls; elsewhere every step observes no motion.
   */
  async scrollWindowUntil(windowId: string, point: Point, request: ScrollUntilRequest, decide?: (observation: ScrollObservation) => Promise<boolean>): Promise<ScrollUntilOutcome> {
    let streak = 0
    let receipt: InputReceipt | undefined
    for (let steps = 1; ; steps++) {
      const before = this.#musicScroll
      const delivered = await this.scrollWindow(windowId, point, request.delta)
      const moved = this.#musicScroll !== before
      receipt ??= delivered
      await delay(Math.min(request.settleMs, 60))
      const capture = await this.captureWindow(windowId)
      const matches = this.#text(capture.bounds)
      const recognized = { matches, text: matches.map(match => match.text).join('\n') }
      streak = moved ? 0 : streak + 1
      const match = request.text ? matches.find(candidate => includes(candidate.text, request.text!)) : undefined
      const outcome = { capture, receipt, recognized, steps }
      if (match)
        return { ...outcome, match, reason: 'text-visible' }
      if (streak >= request.confirmations)
        return { ...outcome, reason: 'end' }
      if (steps >= request.maxSteps)
        return { ...outcome, reason: 'budget' }
      if (decide && await decide({ moved, steps, text: recognized.text }))
        return { ...outcome, reason: 'until' }
    }
  }

  async typeText(text: string): Promise<InputReceipt> {
    await delay(8 * text.length)
    if (this.#focused)
      this.#draft += text
    return { path: 'mock-keyboard' }
  }

  /**
   * Plausible recognizer confidence: faint placeholder text and struck-through
   * rows read worse than large, high-contrast labels.
   */
  #confidence(widget: Widget): number {
    if (widget.role === 'input')
      return this.#draft ? 0.9 : 0.56
    if (widget.id === 'count')
      return 0.99
    if (widget.role === 'button')
      return widget.action === 'increment' ? 0.94 : 0.82
    if (widget.rowIndex !== undefined && this.#todos[widget.rowIndex]!.done)
      return 0.68
    return 0.95
  }

  /** Player bar and scrollbar of the Music window. */
  #decorateMusic(ctx: OffscreenCanvasRenderingContext2D, frame: Rect): void {
    const view = offset(MUSIC_VIEW, frame)
    ctx.fillStyle = '#f1f5f9'
    ctx.fillRect(frame.x, frame.y + 510, frame.width, 70)
    ctx.fillStyle = '#e2e8f0'
    ctx.fillRect(frame.x, frame.y + 510, frame.width, 1)
    const content = SONGS.length * SONG_ROW
    const thumb = Math.max(30, view.height * view.height / content)
    const top = view.y + (view.height - thumb) * (this.#musicScroll / Math.max(1, content - view.height))
    ctx.fillStyle = 'rgba(100, 116, 139, 0.45)'
    roundRect(ctx, { height: thumb, width: 5, x: view.x + view.width - 7, y: top }, 3)
    ctx.fill()
  }

  #display(id?: string): DisplayInfo {
    return DISPLAYS.find(display => display.id === id) ?? DISPLAYS.find(display => display.primary)!
  }

  #displayAt(rect: Rect): DisplayInfo {
    const center = { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 }
    return DISPLAYS.find(display => contains(display.frame, center)) ?? DISPLAYS[0]!
  }

  #hit(point: Point): void {
    for (const window of [...this.#windows].reverse()) {
      if (!contains(window.frame, point))
        continue
      for (const widget of window.widgets()) {
        const rect = offset(widget.rect, window.frame)
        if (!widget.action || !contains(rect, point) || (widget.clip && !contains(offset(widget.clip, window.frame), point)))
          continue
        this.#focused = widget.action === 'focus'
        if (widget.action === 'play' && widget.songIndex !== undefined)
          this.#nowPlaying = widget.songIndex
        if (widget.action === 'increment')
          this.#count += 1
        if (widget.action === 'done' && widget.rowIndex !== undefined)
          this.#todos[widget.rowIndex]!.done = !this.#todos[widget.rowIndex]!.done
        return
      }
      this.#focused = false
      return
    }
  }

  #musicWidgets(): Widget[] {
    const widgets: Widget[] = [
      { id: 'music-header', label: `Liked Songs · ${SONGS.length} songs`, rect: { height: 30, width: 320, x: 16, y: 44 }, role: 'text' },
    ]
    SONGS.forEach((song, index) => {
      const y = MUSIC_VIEW.y + index * SONG_ROW - this.#musicScroll
      widgets.push(
        { action: 'play', clip: MUSIC_VIEW, id: `song-${index}`, label: song.title, rect: { height: 40, width: 260, x: MUSIC_VIEW.x, y }, role: 'text', songIndex: index },
        { action: 'play', clip: MUSIC_VIEW, id: `artist-${index}`, label: song.artist, rect: { height: 40, width: 160, x: MUSIC_VIEW.x + 270, y }, role: 'text', songIndex: index },
      )
    })
    const playing = this.#nowPlaying === null ? undefined : SONGS[this.#nowPlaying]
    widgets.push({ id: 'now-playing', label: playing ? `Now playing: ${playing.title} — ${playing.artist}` : 'Nothing playing', rect: { height: 44, width: 448, x: 16, y: 523 }, role: 'text' })
    return widgets
  }

  #paintDesktop(ctx: OffscreenCanvasRenderingContext2D, bounds: Rect): void {
    for (const display of DISPLAYS) {
      const f = display.frame
      const gradient = ctx.createLinearGradient(f.x, f.y, f.x + f.width, f.y + f.height)
      gradient.addColorStop(0, display.primary ? '#1e293b' : '#1f2937')
      gradient.addColorStop(1, display.primary ? '#4c1d95' : '#0f766e')
      ctx.fillStyle = gradient
      ctx.fillRect(f.x, f.y, f.width, f.height)
      ctx.fillStyle = 'rgba(15, 23, 42, 0.75)'
      ctx.fillRect(f.x, f.y, f.width, 26)
      ctx.fillStyle = '#e2e8f0'
      ctx.font = `13px ${FONT}`
      ctx.fillText(display.name ?? display.id, f.x + 14, f.y + 17)
    }
    void bounds
  }

  #paintWindow(ctx: OffscreenCanvasRenderingContext2D, window: MockWindow): void {
    const f = window.frame
    ctx.save()
    ctx.shadowBlur = 30
    ctx.shadowColor = 'rgba(0,0,0,0.45)'
    ctx.fillStyle = window.background
    roundRect(ctx, f, 12)
    ctx.fill()
    ctx.restore()
    ctx.fillStyle = '#e5e7eb'
    ctx.fillRect(f.x, f.y, f.width, 34)
    for (const [i, color] of ['#ef4444', '#f59e0b', '#22c55e'].entries()) {
      ctx.beginPath()
      ctx.arc(f.x + 20 + i * 20, f.y + 17, 6, 0, Math.PI * 2)
      ctx.fillStyle = color
      ctx.fill()
    }
    ctx.fillStyle = '#374151'
    ctx.font = `600 14px ${FONT}`
    ctx.fillText(window.title ?? '', f.x + 90, f.y + 22)
    window.decorate?.(ctx, f)
    for (const widget of window.widgets()) {
      const r = offset(widget.rect, f)
      const clip = widget.clip && offset(widget.clip, f)
      if (clip && !intersects(clip, r))
        continue
      ctx.save()
      if (clip) {
        ctx.beginPath()
        ctx.rect(clip.x, clip.y, clip.width, clip.height)
        ctx.clip()
      }
      if (widget.role === 'button') {
        ctx.fillStyle = widget.action === 'increment' ? '#4f46e5' : '#e0e7ff'
        roundRect(ctx, r, 8)
        ctx.fill()
        ctx.fillStyle = widget.action === 'increment' ? '#ffffff' : '#3730a3'
        ctx.font = `600 ${widget.action === 'increment' ? 20 : 14}px ${FONT}`
      }
      else if (widget.role === 'input') {
        ctx.strokeStyle = this.#focused ? '#6366f1' : '#cbd5e1'
        ctx.lineWidth = 2
        roundRect(ctx, r, 6)
        ctx.stroke()
        ctx.fillStyle = this.#draft ? '#111827' : '#9ca3af'
        ctx.font = `15px ${FONT}`
      }
      else {
        ctx.fillStyle = '#111827'
        ctx.font = `${widget.id === 'count' ? '700 30px' : '16px'} ${FONT}`
      }
      ctx.textBaseline = 'middle'
      const textX = widget.role === 'button' ? r.x + (r.width - ctx.measureText(widget.label).width) / 2 : r.x + 10
      ctx.fillText(widget.label, textX, r.y + r.height / 2)
      if (widget.role === 'text' && widget.rowIndex !== undefined && this.#todos[widget.rowIndex]!.done) {
        ctx.strokeStyle = '#6b7280'
        ctx.lineWidth = 1.5
        ctx.beginPath()
        ctx.moveTo(r.x + 10, r.y + r.height / 2)
        ctx.lineTo(r.x + 10 + ctx.measureText(widget.label).width, r.y + r.height / 2)
        ctx.stroke()
      }
      ctx.textBaseline = 'alphabetic'
      ctx.restore()
    }
  }

  #render(bounds: Rect, scale: number, source: string): CapturedFrame {
    const width = Math.round(bounds.width * scale)
    const height = Math.round(bounds.height * scale)
    const canvas = new OffscreenCanvas(width, height)
    const ctx = canvas.getContext('2d')!
    ctx.scale(scale, scale)
    ctx.translate(-bounds.x, -bounds.y)
    this.#paintDesktop(ctx, bounds)
    for (const window of this.#windows)
      this.#paintWindow(ctx, window)
    const ref = `mock-cap-${++this.#captureCount}`
    this.#captures.set(ref, canvas)
    // NOTICE(mock-capture-store): like the Runner's capture store, keep only
    // recent captures; live mode renders a display every 900 ms.
    for (const old of this.#captures.keys()) {
      if (this.#captures.size <= 64)
        break
      this.#captures.delete(old)
    }
    return { bounds: { ...bounds }, height, ref, scale, source, width }
  }

  /** Applies a wheel delta; only the Music song list scrolls. Returns whether it moved. */
  #scroll(windowId: string, point: Point, delta: ScrollDelta): boolean {
    if (windowId !== 'w-music' || !contains(MUSIC_VIEW, point))
      return false
    const max = SONGS.length * SONG_ROW - MUSIC_VIEW.height
    const next = Math.min(max, Math.max(0, this.#musicScroll + (delta.dy ?? 0)))
    const moved = next !== this.#musicScroll
    this.#musicScroll = next
    return moved
  }

  /** OCR ground truth: every widget label intersecting `area`, in screen space. */
  /** Text visible in `bounds`, or only in its `region` (fractions of `bounds`) when given. */
  /** Text visible in `bounds`, limited to the screen-space `within` area. */
  #text(bounds: Rect, within?: Rect): TextMatch[] {
    const area = within ? clip(within, bounds) : bounds
    const matches: TextMatch[] = []
    const measure = new OffscreenCanvas(1, 1).getContext('2d')!
    for (const window of this.#windows) {
      measure.font = `600 14px ${FONT}`
      const titleRect = { height: 18, width: measure.measureText(window.title ?? '').width, x: window.frame.x + 90, y: window.frame.y + 8 }
      if (intersects(area, titleRect))
        matches.push({ bounds: titleRect, confidence: 0.99, text: window.title ?? '' })
      for (const widget of window.widgets()) {
        const r = offset(widget.rect, window.frame)
        measure.font = `${widget.role === 'button' ? '600 14px' : widget.id === 'count' ? '700 30px' : '16px'} ${FONT}`
        const width = measure.measureText(widget.label).width
        const fontSize = widget.id === 'count' ? 30 : 16
        const textRect = widget.role === 'button'
          ? { height: fontSize + 4, width, x: r.x + (r.width - width) / 2, y: r.y + (r.height - fontSize) / 2 - 2 }
          : { height: fontSize + 4, width, x: r.x + 10, y: r.y + (r.height - fontSize) / 2 - 2 }
        // Text cut off by a scroll viewport is not readable.
        const clip = widget.clip && offset(widget.clip, window.frame)
        if (clip && (!contains(clip, textRect) || !contains(clip, { x: textRect.x + textRect.width, y: textRect.y + textRect.height })))
          continue
        if (widget.label && intersects(area, textRect))
          matches.push({ bounds: textRect, confidence: this.#confidence(widget), text: widget.label })
      }
    }
    return matches
  }

  #todoWidgets(): Widget[] {
    const widgets: Widget[] = [
      { action: 'focus', id: 'input', label: this.#draft || 'Add a todo…', rect: { height: 40, width: 700, x: 30, y: 54 }, role: 'input' },
    ]
    this.#todos.forEach((todo, index) => {
      const y = 116 + index * 52
      widgets.push(
        { id: `row-${index}`, label: todo.text, rect: { height: 40, width: 520, x: 30, y }, role: 'text', rowIndex: index },
        { action: 'done', id: `done-${index}`, label: todo.done ? 'Undo' : 'Done', rect: { height: 36, width: 120, x: 610, y: y + 2 }, role: 'button', rowIndex: index },
      )
    })
    return widgets
  }

  #window(id: string): MockWindow {
    const window = this.#windows.find(candidate => candidate.id === id)
    if (!window)
      throw new Error(`Mock window ${id} does not exist`)
    return window
  }
}

function contains(rect: Rect, point: Point): boolean {
  return point.x >= rect.x && point.x <= rect.x + rect.width && point.y >= rect.y && point.y <= rect.y + rect.height
}

async function delay(ms: number): Promise<void> {
  await new Promise(resolve => setTimeout(resolve, ms))
}

function intersects(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

function offset(rect: Rect, by: Rect): Rect {
  return { height: rect.height, width: rect.width, x: rect.x + by.x, y: rect.y + by.y }
}

function roundRect(ctx: OffscreenCanvasRenderingContext2D, rect: Rect, radius: number): void {
  ctx.beginPath()
  ctx.roundRect(rect.x, rect.y, rect.width, rect.height, radius)
}

function toInfo(window: MockWindow): WindowInfo {
  const { app, bundleId, frame, id, pid, title } = window
  return { app, bundleId, frame: { ...frame }, id, pid, title }
}
