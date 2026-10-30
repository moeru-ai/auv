import { Tooltip } from '@base-ui/react/tooltip'

import { iconButtonClassName } from './icon-button-class'
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
  return (
    <Tooltip.Root>
      <Tooltip.Trigger
        render={(
          <button
            aria-label={hint}
            className={iconButtonClassName({ active, label, tone })}
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
