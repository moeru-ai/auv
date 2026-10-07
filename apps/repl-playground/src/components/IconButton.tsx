import { Tooltip } from '@base-ui/react/tooltip'

import { Kbd } from './Kbd'

/** Icon-only button with a tooltip; used by the activity bar, debug bar and panel headers. */
export function IconButton({ active, disabled, hint, icon, keys, label, onClick, side = 'top', tone = 'ghost' }: {
  active?: boolean
  disabled?: boolean
  hint: string
  icon: string
  keys?: string
  label?: string
  onClick: () => void
  side?: 'bottom' | 'left' | 'right' | 'top'
  tone?: 'ghost' | 'primary'
}) {
  const base = label ? 'h-7 gap-1.5 px-2.5' : 'size-7 justify-center'
  const style = tone === 'primary'
    ? 'bg-accent text-white hover:bg-accent-strong'
    : active ? 'bg-accent/18 text-accent' : 'text-fg-muted hover:(bg-surface-2 text-fg)'
  return (
    <Tooltip.Root>
      <Tooltip.Trigger
        render={(
          <button
            aria-label={hint}
            className={`text-[12.5px] font-medium rounded-md inline-flex cursor-pointer transition-colors items-center disabled:(opacity-35 cursor-not-allowed) ${base}  ${style}`}
            disabled={disabled}
            onClick={onClick}
            type="button"
          />
        )}
      >
        <span className={`${icon} text-[15px]`} />
        {label}
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Positioner side={side} sideOffset={8}>
          <Tooltip.Popup className="text-[12px] text-fg px-2 py-1 border border-line rounded-md bg-surface-2 shadow-lg z-50">
            {hint}
            {keys && <Kbd className="ml-2">{keys}</Kbd>}
          </Tooltip.Popup>
        </Tooltip.Positioner>
      </Tooltip.Portal>
    </Tooltip.Root>
  )
}
