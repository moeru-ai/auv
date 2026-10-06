import type { Pin } from '../store'

import { actions, usePlayground } from '../store'
import { ValuePreview } from './HoverCard'
import { ResourcePreview } from './ResourcePreview'

/**
 * Floating cards pinned from hovers. Each card is fixed to one moment
 * (seq), so several can be laid side by side to compare evidence.
 */
export function PinLayer() {
  const pins = usePlayground(state => state.pins)
  return (
    <div className="pointer-events-none inset-0 fixed z-40">
      {pins.map(pin => <PinnedCard key={pin.id} pin={pin} />)}
    </div>
  )
}

function PinnedCard({ pin }: { pin: Pin }) {
  const resource = usePlayground(state => (pin.ref ? state.resources[pin.ref] : undefined))
  const atCursor = usePlayground(state => state.cursor === pin.seq)

  const startDrag = (event: React.PointerEvent) => {
    event.preventDefault()
    const start = { pinX: pin.x, pinY: pin.y, x: event.clientX, y: event.clientY }
    const move = (moveEvent: PointerEvent) => {
      const x = Math.min(window.innerWidth - 80, Math.max(0, start.pinX + moveEvent.clientX - start.x))
      const y = Math.min(window.innerHeight - 40, Math.max(0, start.pinY + moveEvent.clientY - start.y))
      actions.movePin(pin.id, x, y)
    }
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  return (
    <div
      className={`border rounded-xl bg-surface-1 flex flex-col max-w-[440px] min-w-[260px] pointer-events-auto shadow-2xl absolute overflow-hidden ${atCursor ? 'border-[var(--o-hover)]' : 'border-line'}`}
      onMouseEnter={() => pin.ref && usePlayground.setState({ hoveredRef: pin.ref })}
      onMouseLeave={() => usePlayground.setState({ hoveredRef: null })}
      style={{ left: pin.x, top: pin.y }}
    >
      <div
        className="pl-2.5 pr-1 border-b border-line bg-surface-2 flex gap-2 h-8 cursor-grab items-center active:cursor-grabbing"
        onPointerDown={startDrag}
      >
        <span className="i-lucide-pin text-[12px] text-accent" />
        <span className="text-[12px] text-fg font-mono flex-1 truncate">{pin.label}</span>
        {pin.seq >= 0 && (
          <button
            className="text-[10.5px] text-fg-muted font-mono px-1.5 rounded h-5 cursor-pointer hover:(text-fg bg-surface-0)"
            onClick={() => actions.setCursor(pin.seq)}
            onPointerDown={event => event.stopPropagation()}
            title="Move the time cursor to this moment"
            type="button"
          >
            {`seq ${pin.seq}`}
          </button>
        )}
        <button
          aria-label="Close pinned card"
          className="text-fg-subtle rounded flex size-6 cursor-pointer items-center justify-center hover:(text-fg bg-surface-0)"
          onClick={() => actions.removePin(pin.id)}
          onPointerDown={event => event.stopPropagation()}
          type="button"
        >
          <span className="i-lucide-x text-[13px]" />
        </button>
      </div>
      <div className="text-[12.5px] p-3 max-h-[60vh] overflow-auto">
        {resource ? <ResourcePreview resource={resource} /> : <ValuePreview value={pin.value} />}
      </div>
    </div>
  )
}
