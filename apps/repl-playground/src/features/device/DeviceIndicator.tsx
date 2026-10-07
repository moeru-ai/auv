import { Popover } from '@base-ui/react/popover'
import { useState } from 'react'

import { AddDeviceForm } from './AddDeviceForm'
import { DevicePicker } from './DevicePicker'
import { connectionName, MOCK_ID, PLATFORMS, useDevices } from './devices'

type View = 'add' | 'pick'

/**
 * Remote indicator in the status bar's left corner (VS Code style): shows the
 * active Device and opens the device picker.
 */
export function DeviceIndicator() {
  const [open, setOpen] = useState(false)
  const [view, setView] = useState<View>('pick')
  const active = useDevices(state => state.active)
  const states = useDevices(state => state.states)
  const saved = useDevices(state => state.saved)
  const connection = active && active.connectionId !== MOCK_ID ? saved.find(candidate => candidate.id === active.connectionId) : undefined
  const device = active && states[active.connectionId]?.devices.find(candidate => candidate.id === active.deviceId)
  const connecting = Object.values(states).some(state => state.status === 'connecting')
  const others = Object.entries(states).filter(([id, state]) => id !== MOCK_ID && id !== active?.connectionId && state.status === 'connected').length

  return (
    <Popover.Root
      onOpenChange={(next) => {
        setOpen(next)
        if (next)
          setView('pick')
      }}
      open={open}
    >
      <Popover.Trigger
        aria-label="Switch device"
        className="text-[12px] text-white font-medium px-2.5 bg-accent flex shrink-0 gap-1.5 h-full cursor-pointer transition-colors items-center data-[popup-open]:bg-accent-strong hover:bg-accent-strong"
        title={device ? `Active device: ${device.name}${connection ? ` on ${connectionName(connection)}` : ''} — click to switch` : 'Connect a device'}
      >
        <span className={`${connecting && !device ? 'i-ph-circle-notch animate-spin' : device ? PLATFORMS[device.platform].icon : 'i-ph-link'} text-[14px]`} />
        <span className="max-w-60 truncate">{device ? device.name : connecting ? 'Connecting…' : 'No device'}</span>
        {connection && <span className="opacity-75 max-w-40 truncate">{`· ${connectionName(connection)}`}</span>}
        {others > 0 && <span className="text-[10.5px] px-1 rounded bg-white/20" title={`${others} more connected`}>{`+${others}`}</span>}
        {connecting && device && <span className="i-ph-circle-notch animate-spin" />}
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner align="start" side="top" sideOffset={6}>
          <Popover.Popup className="ml-1.5 border border-line rounded-xl bg-surface-1 w-[min(460px,calc(100vw-24px))] shadow-2xl transition-[opacity,transform] duration-120 overflow-hidden data-[ending-style]:(opacity-0 translate-y-1) data-[starting-style]:(opacity-0 translate-y-1)">
            {view === 'pick'
              ? <DevicePicker onAdd={() => setView('add')} onClose={() => setOpen(false)} />
              : (
                  <>
                    <div className="pl-1.5 pr-3 border-b border-line flex gap-1.5 h-10 items-center">
                      <button aria-label="Back to devices" className="text-fg-muted rounded-md flex size-7 cursor-pointer items-center justify-center hover:(text-fg bg-surface-2)" onClick={() => setView('pick')} type="button">
                        <span className="i-ph-caret-left text-[16px]" />
                      </button>
                      <Popover.Title className="text-[13px] font-semibold m-0">Add a device</Popover.Title>
                    </div>
                    <AddDeviceForm onDone={() => setOpen(false)} />
                  </>
                )}
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  )
}
