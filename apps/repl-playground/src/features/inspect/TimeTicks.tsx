import { actions } from '../../store'

export interface Tick {
  label: string
  seq: number
  title: string
  tone?: 'bad' | 'default'
}

/**
 * A compact strip of moments (one per loop iteration / capture / call).
 * Hovering previews a moment; clicking also moves the global time cursor.
 * Long histories collapse into a slider.
 */
export function TimeTicks({ cursor, onSelect, selected, ticks }: {
  cursor: number
  onSelect: (index: number) => void
  selected: number
  ticks: Tick[]
}) {
  // A single tick still renders so the row height is stable when more arrive.
  if (ticks.length === 0)
    return null
  if (ticks.length > 40) {
    return (
      <div className="flex gap-2 items-center">
        <input
          className="accent-[var(--c-accent)] flex-1 h-1 cursor-pointer"
          max={ticks.length - 1}
          min={0}
          onChange={event => onSelect(Number(event.target.value))}
          onPointerUp={() => actions.setCursor(ticks[selected]!.seq)}
          type="range"
          value={selected}
        />
        <span className="text-[11px] text-fg-muted font-mono text-right w-24">{`${ticks[selected]!.label} / ${ticks.length}`}</span>
      </div>
    )
  }
  return (
    // One row (scrolls horizontally) so new ticks never change the card height.
    <div className="flex flex-nowrap gap-1 [scrollbar-width:none] items-center overflow-x-auto">
      {ticks.map((tick, index) => {
        const isSelected = index === selected
        const atCursor = tick.seq <= cursor && (ticks[index + 1]?.seq ?? Infinity) > cursor
        return (
          <button
            className={[
              'h-5 min-w-6 shrink-0 cursor-pointer rounded px-1 text-[10.5px] font-mono transition-colors',
              isSelected ? 'bg-accent text-white' : tick.tone === 'bad' ? 'bg-bad/15 text-bad' : 'bg-surface-2 text-fg-muted hover:text-fg',
              atCursor && !isSelected ? 'ring-1 ring-[var(--o-hover)]' : '',
            ].join(' ')}
            key={tick.seq}
            onClick={() => actions.setCursor(tick.seq)}
            onMouseEnter={() => onSelect(index)}
            title={`${tick.title} — click to move the time cursor here`}
            type="button"
          >
            {tick.label}
          </button>
        )
      })}
    </div>
  )
}
