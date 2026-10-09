import type { ConnectionState, DeviceSummary, SavedConnection } from './devices'

import { Menu } from '@base-ui/react/menu'
import { useEffect, useMemo, useRef, useState } from 'react'

import { copyText } from '../../clipboard'
import { IconButton } from '../../components/IconButton'
import { Kbd } from '../../components/Kbd'
import { usePlayground } from '../../store'
import { activateDevice, connectionName, disconnect, forgetConnection, MOCK_ID, PLATFORMS, useDevices } from './devices'

/**
 * A keyboard-selectable row of the picker. Device rows carry their daemon
 * connection (absent for the mock desktop); a daemon that is not connected
 * shows as one `connect` row.
 */
type Item
  = | { connection: SavedConnection, kind: 'connect' }
    | { connection?: SavedConnection, device: DeviceSummary, kind: 'device' }
    | { kind: 'add' }

/**
 * Quick-pick list of every known Device. Several daemons can stay connected;
 * picking a Device makes it the one scripts and the canvas use.
 */
export function DevicePicker({ onAdd, onClose }: { onAdd: () => void, onClose: () => void }) {
  const active = useDevices(state => state.active)
  const saved = useDevices(state => state.saved)
  const states = useDevices(state => state.states)
  const runStatus = usePlayground(state => state.run.status)
  const locked = runStatus === 'running' || runStatus === 'paused' || runStatus === 'compiling'
  const [query, setQuery] = useState('')
  const [highlight, setHighlight] = useState(0)
  const [pending, setPending] = useState<string>()
  const [error, setError] = useState<string>()
  const listRef = useRef<HTMLDivElement>(null)

  const items = useMemo(() => buildItems(saved, states, query), [saved, states, query])
  const index = Math.min(highlight, items.length - 1)

  useEffect(() => {
    listRef.current?.querySelector('[data-highlighted="true"]')?.scrollIntoView({ block: 'nearest' })
  }, [index])

  const choose = async (item: Item | undefined) => {
    if (!item)
      return
    if (item.kind === 'add') {
      onAdd()
      return
    }
    if (locked)
      return
    const key = itemKey(item)
    setPending(key)
    setError(undefined)
    try {
      if (item.kind === 'device')
        await activateDevice(item.connection?.id ?? MOCK_ID, item.device.id)
      else
        await activateDevice(item.connection.id)
      onClose()
    }
    catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    }
    finally {
      setPending(undefined)
    }
  }

  return (
    <div className="flex flex-col max-h-[min(560px,70vh)]">
      <div className="px-3 border-b border-line flex shrink-0 gap-2 h-10 items-center">
        <span className="i-ph-magnifying-glass text-fg-subtle" />
        <input
          aria-label="Search devices"
          autoFocus
          className="text-[13px] text-fg outline-none bg-transparent flex-1 placeholder:text-fg-subtle"
          onChange={(event) => {
            setQuery(event.target.value)
            setHighlight(0)
          }}
          onKeyDown={(event) => {
            if (event.key === 'ArrowDown')
              setHighlight((index + 1) % Math.max(items.length, 1))
            else if (event.key === 'ArrowUp')
              setHighlight((index - 1 + items.length) % Math.max(items.length, 1))
            else if (event.key === 'Enter')
              void choose(items[index])
            else
              return
            event.preventDefault()
          }}
          placeholder="Switch to a device, or connect a new one…"
          value={query}
        />
      </div>

      {locked && (
        <Banner icon="i-ph-info" tone="warn">Stop the current run to switch devices.</Banner>
      )}
      {error && (
        <Banner icon="i-ph-x-circle" tone="bad">{error}</Banner>
      )}

      <div className="py-1.5 flex flex-1 flex-col gap-0.5 min-h-0 overflow-auto" ref={listRef} role="listbox">
        {items.map((item, position) => {
          const key = itemKey(item)
          return (
            <Row
              disabled={locked && item.kind !== 'add'}
              highlighted={position === index}
              item={item}
              key={key}
              locked={locked}
              onChoose={() => void choose(item)}
              onHover={() => setHighlight(position)}
              pending={pending === key}
              selected={item.kind === 'device' && active?.connectionId === (item.connection?.id ?? MOCK_ID) && active.deviceId === item.device.id}
              state={item.kind === 'add' ? undefined : item.connection ? states[item.connection.id] : undefined}
            />
          )
        })}
        {items.length === 0 && <div className="text-fg-subtle px-3 py-6 text-center">{`No device matches “${query}”.`}</div>}
      </div>

      <div className="text-[11px] text-fg-subtle px-3 border-t border-line flex shrink-0 gap-3 h-8 items-center">
        <span className="flex gap-1 items-center">
          <Kbd>↑</Kbd>
          <Kbd>↓</Kbd>
          navigate
        </span>
        <span className="flex gap-1 items-center">
          <Kbd>↵</Kbd>
          switch
        </span>
        <span className="flex gap-1 items-center">
          <Kbd>esc</Kbd>
          close
        </span>
      </div>
    </div>
  )
}

function Banner({ children, icon, tone }: { children: React.ReactNode, icon: string, tone: 'bad' | 'warn' }) {
  return (
    <div className={`text-[12px] mx-2 mt-2 px-2.5 py-1.5 rounded-md flex gap-2 items-start ${tone === 'bad' ? 'text-bad bg-bad/10' : 'text-warn bg-warn/10'}`} role={tone === 'bad' ? 'alert' : 'status'}>
      <span className={`${icon} mt-0.5 shrink-0`} />
      <span className="[overflow-wrap:anywhere]">{children}</span>
    </div>
  )
}

function buildItems(saved: SavedConnection[], states: Record<string, ConnectionState>, query: string): Item[] {
  const needle = query.trim().toLowerCase()
  const matches = (...texts: Array<string | undefined>) => !needle || texts.some(text => text?.toLowerCase().includes(needle))
  const items: Item[] = (states[MOCK_ID]?.devices ?? [])
    .filter(device => matches(device.name, 'mock'))
    .map(device => ({ device, kind: 'device' }))
  for (const connection of saved) {
    const state = states[connection.id] ?? { devices: [], status: 'disconnected' }
    const connectionMatches = matches(connectionName(connection), connection.endpoint)
    if (state.status !== 'connected') {
      if (connectionMatches)
        items.push({ connection, kind: 'connect' })
      continue
    }
    for (const device of state.devices) {
      if (connectionMatches || matches(device.name, PLATFORMS[device.platform].label))
        items.push({ connection, device, kind: 'device' })
    }
  }
  if (matches('add', 'pair', 'connect', 'new', 'device'))
    items.push({ kind: 'add' })
  return items
}

/** Disconnect for a connected daemon, plus a menu with the rarer, destructive actions. */
function ConnectionActions({ connected, connection, locked }: { connected: boolean, connection: SavedConnection, locked: boolean }) {
  return (
    // Row clicks switch devices; the actions must not.
    <span className="flex shrink-0 gap-0.5 items-center" onClick={event => event.stopPropagation()}>
      {connected && (
        <IconButton disabled={locked} hint={`Disconnect ${connectionName(connection)}`} icon="i-ph-plugs" onClick={() => void disconnect(connection.id)} side="left" />
      )}
      <Menu.Root>
        <Menu.Trigger
          aria-label="More actions"
          className="text-fg-muted rounded-md flex size-7 cursor-pointer transition-colors items-center justify-center data-[popup-open]:(text-fg bg-surface-2) hover:(text-fg bg-surface-2)"
        >
          <span className="i-ph-dots-three text-[16px]" />
        </Menu.Trigger>
        <Menu.Portal>
          <Menu.Positioner align="end" side="bottom" sideOffset={4}>
            <Menu.Popup className="p-1 outline-none border border-line rounded-lg bg-surface-1 min-w-44 shadow-xl z-50">
              <MenuItem icon="i-ph-copy" onClick={() => void copyText(connection.endpoint)}>Copy endpoint</MenuItem>
              <MenuItem disabled={locked} icon="i-ph-trash" onClick={() => void forgetConnection(connection.id)} tone="bad">Forget daemon</MenuItem>
            </Menu.Popup>
          </Menu.Positioner>
        </Menu.Portal>
      </Menu.Root>
    </span>
  )
}

function itemKey(item: Item): string {
  if (item.kind === 'add')
    return 'add'
  if (item.kind === 'connect')
    return `connect:${item.connection.id}`
  return `${item.connection?.id ?? MOCK_ID}/${item.device.id}`
}

function MenuItem({ children, disabled, icon, onClick, tone }: { children: React.ReactNode, disabled?: boolean, icon: string, onClick: () => void, tone?: 'bad' }) {
  return (
    <Menu.Item
      className={`text-[12.5px] px-2 outline-none rounded-md flex gap-2 h-7 cursor-pointer select-none items-center data-[highlighted]:bg-surface-2 data-[disabled]:(opacity-40 cursor-not-allowed) ${tone === 'bad' ? 'text-bad' : 'text-fg'}`}
      disabled={disabled}
      onClick={onClick}
    >
      <span className={`${icon} text-[14px]`} />
      {children}
    </Menu.Item>
  )
}

function Row({ disabled, highlighted, item, locked, onChoose, onHover, pending, selected, state }: {
  disabled: boolean
  highlighted: boolean
  item: Item
  locked: boolean
  onChoose: () => void
  onHover: () => void
  pending: boolean
  selected: boolean
  /** State of the row's daemon connection; absent for the mock and the add row. */
  state?: ConnectionState
}) {
  const content = (() => {
    if (item.kind === 'add')
      return { detail: 'Local daemon, or pair with one on another machine', icon: 'i-ph-plus', title: 'Add a device…' }
    if (item.kind === 'connect') {
      const failed = state?.status === 'error'
      return {
        detail: failed ? state.error : state?.status === 'connecting' ? 'Connecting…' : 'Not connected · press ↵ to connect',
        failed,
        icon: 'i-ph-plugs',
        title: connectionName(item.connection),
      }
    }
    const platform = PLATFORMS[item.device.platform]
    return {
      detail: item.connection ? `${platform.label} · ${connectionName(item.connection)}` : platform.label,
      icon: platform.icon,
      title: item.device.name,
    }
  })()
  const busy = pending || state?.status === 'connecting'
  const background = selected
    ? highlighted ? 'bg-accent/20' : 'bg-accent/12'
    : highlighted ? 'bg-surface-2' : ''

  return (
    <div
      aria-disabled={disabled}
      aria-selected={selected}
      className={`mx-1.5 px-2 py-1.5 rounded-md flex gap-2.5 min-h-11 cursor-pointer transition-colors items-center ${background}  ${disabled ? 'opacity-50 cursor-not-allowed' : ''}`}
      data-highlighted={highlighted}
      onClick={disabled ? undefined : onChoose}
      onMouseMove={onHover}
      role="option"
    >
      <span className={`rounded-md flex shrink-0 size-7 items-center justify-center relative ${selected ? 'text-white bg-accent' : item.kind === 'add' ? 'text-accent bg-accent/12' : 'text-fg-muted bg-surface-0'}`}>
        <span className={`${busy ? 'i-ph-circle-notch animate-spin' : content.icon} text-[16px]`} />
        {state && <StatusDot status={state.status} />}
      </span>
      <span className="flex flex-1 flex-col min-w-0">
        <span className={`text-[13px] truncate ${selected ? 'text-fg font-medium' : item.kind === 'add' ? 'text-accent' : 'text-fg'}`}>{content.title}</span>
        <span className={`text-[11.5px] truncate ${'failed' in content && content.failed ? 'text-bad' : 'text-fg-subtle'}`}>{content.detail}</span>
      </span>
      {selected && <span className="i-ph-check-circle-fill text-[16px] text-accent shrink-0" title="Active device" />}
      {item.kind !== 'add' && item.connection && (highlighted || selected) && (
        <ConnectionActions connected={state?.status === 'connected'} connection={item.connection} locked={locked} />
      )}
    </div>
  )
}

/** Connection status as a badge on the row icon's corner. */
function StatusDot({ status }: { status: ConnectionState['status'] }) {
  const style = { connected: 'bg-good', connecting: 'bg-warn animate-pulse', disconnected: 'bg-fg-subtle', error: 'bg-bad' }[status]
  return <span className={`rounded-full size-2 ring-2 ring-surface-1 absolute -bottom-0.5 -right-0.5 ${style}`} title={status} />
}
