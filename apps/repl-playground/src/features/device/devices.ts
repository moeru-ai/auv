import type { Device } from '@auv-js/sdk'

import type { Backend } from '../../backend/types'

import { create } from 'zustand'

import { createAuvBackend } from '../../backend/auv'
import { MockBackend } from '../../backend/mock'
import { activateBackend, session } from '../../runtime/session'

const STORAGE_KEY = 'auv-playground:devices'

/** Built-in connection and Device ID of the in-browser mock desktop. */
export const MOCK_ID = 'mock'

export const DEFAULT_ENDPOINT = 'http://127.0.0.1:9847'

/** Which Device scripts, the canvas and live view talk to. */
export interface ActiveDevice {
  connectionId: string
  deviceId: string
}

/** Runtime state of one connection; not persisted. */
export interface ConnectionState {
  /** Devices the daemon reported on the last successful connect. */
  devices: DeviceSummary[]
  error?: string
  status: ConnectionStatus
}

export type ConnectionStatus = 'connected' | 'connecting' | 'disconnected' | 'error'

export interface DevicesState {
  active: ActiveDevice | null
  /** Remembered daemon endpoints, in the order they were added. */
  saved: SavedConnection[]
  /** Runtime state per connection ID, including the mock. */
  states: Record<string, ConnectionState>
}

/** A Device as the picker shows it: an AUV Device, or the built-in mock desktop. */
export interface DeviceSummary {
  hostname?: string
  id: string
  /** Whether the Device is the daemon's own machine. */
  local: boolean
  name: string
  platform: 'mock' | Device['platform']
}

/** An AUV daemon endpoint this browser remembers. */
export interface SavedConnection {
  /** Reconnect on the next page load; cleared by an explicit disconnect. */
  autoConnect: boolean
  /** Paired Device credential; absent for a local loopback daemon without a pairing store. */
  credential?: string
  endpoint: string
  id: string
  /** User-facing name; the endpoint host is shown when absent. */
  name?: string
}

interface StoredDevices {
  active?: ActiveDevice | null
  saved?: SavedConnection[]
}

/** Icon and name per Device platform. */
export const PLATFORMS: Record<DeviceSummary['platform'], { icon: string, label: string }> = {
  linux: { icon: 'i-ph-linux-logo', label: 'Linux' },
  macos: { icon: 'i-ph-apple-logo', label: 'macOS' },
  mock: { icon: 'i-ph-game-controller', label: 'In-browser' },
  unspecified: { icon: 'i-ph-desktop', label: 'Unknown OS' },
  windows: { icon: 'i-ph-windows-logo', label: 'Windows' },
}

const MOCK_DEVICE: DeviceSummary = { id: MOCK_ID, local: true, name: 'Mock desktop', platform: 'mock' }

const stored = loadStored()

export const useDevices = create<DevicesState>(() => ({
  active: null,
  saved: stored.saved ?? [],
  states: { [MOCK_ID]: { devices: [MOCK_DEVICE], status: 'connected' } },
}))

const set = useDevices.setState
const get = useDevices.getState

/**
 * Open backends by `connectionId/deviceId`. Switching the active Device keeps
 * the others open so switching back is instant; disconnecting a connection
 * disposes all of its backends.
 */
const backends = new Map<string, Backend>()

let restoring: Promise<void> | undefined

/** Makes a Device active, connecting its daemon first when needed. */
export async function activateDevice(connectionId: string, deviceId?: string): Promise<void> {
  if (session.running)
    throw new Error('Stop the current run before switching devices.')
  const connected = connectionId === MOCK_ID || get().states[connectionId]?.status === 'connected'
  const defaultDevice = connected
    ? get().states[connectionId]?.devices.find(device => device.local)?.id ?? get().states[connectionId]?.devices[0]?.id
    : await connectDaemon(connectionId)
  const target = deviceId ?? defaultDevice
  if (!target)
    throw new Error('The daemon reported no Devices')
  // The SDK falls back to the daemon's local Device for an unknown ID.
  if (!get().states[connectionId]?.devices.some(device => device.id === target))
    throw new Error(`The daemon no longer reports Device ${target}`)
  const backend = await backendFor(connectionId, target)
  const active = { connectionId, deviceId: target }
  set({ active })
  persist()
  await activateBackend(backend)
}

/** Remembers a daemon endpoint (or updates the one with the same endpoint) and activates its default Device. */
export async function addConnection(connection: Omit<SavedConnection, 'autoConnect' | 'id'>): Promise<void> {
  const existing = get().saved.find(candidate => candidate.endpoint === connection.endpoint)
  const id = existing?.id ?? crypto.randomUUID()
  // A changed credential or endpoint needs a fresh connection.
  if (existing)
    await closeConnection(id)
  const next: SavedConnection = { ...existing, ...connection, autoConnect: true, id }
  set(state => ({ saved: existing ? state.saved.map(saved => saved.id === id ? next : saved) : [...state.saved, next] }))
  persist()
  try {
    await activateDevice(id)
  }
  catch (error) {
    // The entry stays so the user can retry or fix it from the picker, but an
    // active Device on the closed connection must not stay selected.
    if (get().active?.connectionId === id)
      await activateDevice(MOCK_ID, MOCK_ID)
    throw error
  }
}

/** Display name of a saved connection. */
export function connectionName(connection: SavedConnection): string {
  if (connection.name)
    return connection.name
  try {
    return new URL(connection.endpoint).host
  }
  catch {
    return connection.endpoint
  }
}

/** Closes a daemon connection; if it held the active Device, the mock desktop takes over. */
export async function disconnect(connectionId: string): Promise<void> {
  set(state => ({ saved: state.saved.map(saved => saved.id === connectionId ? { ...saved, autoConnect: false } : saved) }))
  await closeConnection(connectionId)
  if (get().active?.connectionId === connectionId)
    await activateDevice(MOCK_ID, MOCK_ID)
  persist()
}

/** Disconnects and forgets a saved daemon, including its credential. */
export async function forgetConnection(connectionId: string): Promise<void> {
  await disconnect(connectionId)
  set(state => ({
    saved: state.saved.filter(saved => saved.id !== connectionId),
    states: Object.fromEntries(Object.entries(state.states).filter(([id]) => id !== connectionId)),
  }))
  persist()
}

/** Replaces the mock desktop with a fresh one; it keeps state across runs like a real desktop. */
export async function resetMockDesktop(): Promise<void> {
  const key = backendKey(MOCK_ID, MOCK_ID)
  await backends.get(key)?.dispose().catch(() => {})
  backends.delete(key)
  const { active } = get()
  if (active?.connectionId === MOCK_ID)
    await activateBackend(await backendFor(MOCK_ID, MOCK_ID))
}

/**
 * Page load: restores the previously active Device (the mock desktop when
 * there is none or it is unreachable) and reconnects the other daemons that
 * were connected last time in the background.
 */
export async function restoreDevices(): Promise<void> {
  // NOTICE(strict-mode-effects): React StrictMode runs mount effects twice in
  // development; a second restore would open duplicate daemon connections.
  restoring ??= restore()
  await restoring
}

async function backendFor(connectionId: string, deviceId: string): Promise<Backend> {
  const key = backendKey(connectionId, deviceId)
  const open = backends.get(key)
  if (open)
    return open
  let backend: Backend
  if (connectionId === MOCK_ID) {
    backend = new MockBackend()
  }
  else {
    const connection = savedConnection(connectionId)
    backend = (await createAuvBackend({ credential: connection.credential, deviceId, endpoint: connection.endpoint })).backend
  }
  backends.set(key, backend)
  return backend
}

function backendKey(connectionId: string, deviceId: string): string {
  return `${connectionId}/${deviceId}`
}

async function closeConnection(connectionId: string): Promise<void> {
  const prefix = `${connectionId}/`
  const closing = [...backends].filter(([key]) => key.startsWith(prefix))
  for (const [key] of closing)
    backends.delete(key)
  await Promise.allSettled(closing.map(async ([, backend]) => await backend.dispose()))
  if (get().states[connectionId])
    patchState(connectionId, { error: undefined, status: 'disconnected' })
}

/** Connects a saved daemon and records its Devices; returns the default Device's ID. */
async function connectDaemon(connectionId: string): Promise<string> {
  if (connectionId === MOCK_ID)
    return MOCK_ID
  const connection = savedConnection(connectionId)
  patchState(connectionId, { error: undefined, status: 'connecting' })
  try {
    const { backend, device, devices } = await createAuvBackend({ credential: connection.credential, endpoint: connection.endpoint })
    const key = backendKey(connectionId, device.id)
    // A reconnect replaces the previous backend for the same Device.
    await backends.get(key)?.dispose().catch(() => {})
    backends.set(key, backend)
    patchState(connectionId, { devices: devices.map(summarize), status: 'connected' })
    set(state => ({ saved: state.saved.map(saved => saved.id === connectionId ? { ...saved, autoConnect: true } : saved) }))
    persist()
    return device.id
  }
  catch (error) {
    patchState(connectionId, { error: messageOf(error), status: 'error' })
    throw error
  }
}

// NOTICE(credential-storage): paired Device credentials are kept in
// localStorage for this local development tool. Any script on this origin can
// read them; do not host the playground on a shared origin with real
// credentials. See README "Sandboxing".
function loadStored(): StoredDevices {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (raw)
      return JSON.parse(raw) as StoredDevices
  }
  catch {}
  return {}
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function patchState(connectionId: string, patch: Partial<ConnectionState>): void {
  set(state => ({
    states: {
      ...state.states,
      [connectionId]: { ...(state.states[connectionId] ?? { devices: [], status: 'disconnected' }), ...patch },
    },
  }))
}

function persist(): void {
  const { active, saved } = get()
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ active, saved } satisfies StoredDevices))
  }
  catch {}
}

async function restore(): Promise<void> {
  const previous = stored.active
  for (const connection of get().saved) {
    if (connection.autoConnect && connection.id !== previous?.connectionId)
      void connectDaemon(connection.id).catch(() => {})
  }
  if (previous && previous.connectionId !== MOCK_ID && get().saved.some(connection => connection.id === previous.connectionId)) {
    try {
      await activateDevice(previous.connectionId, previous.deviceId)
      return
    }
    catch (error) {
      console.warn('Could not restore the previous device', error)
    }
  }
  await activateDevice(MOCK_ID, MOCK_ID)
}

function savedConnection(connectionId: string): SavedConnection {
  const connection = get().saved.find(candidate => candidate.id === connectionId)
  if (!connection)
    throw new Error(`Unknown connection ${connectionId}`)
  return connection
}

function summarize(device: Device): DeviceSummary {
  const hostname = device.labels.hostname
  // NOTICE(device-name): a local daemon may report an empty Device name.
  return { hostname, id: device.id, local: device.local, name: device.name || hostname || device.id.slice(0, 12), platform: device.platform }
}
