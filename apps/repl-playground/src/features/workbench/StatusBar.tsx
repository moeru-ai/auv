import { usePlayground } from '../../store'
import { DeviceIndicator } from '../device/DeviceIndicator'

/** Bottom status bar: the active Device on the left, desktop facts on the right. */
export function StatusBar() {
  const displays = usePlayground(state => state.displays.length)
  const live = usePlayground(state => state.live)
  const runId = usePlayground(state => state.run.runId)
  const replay = usePlayground(state => state.run.mode === 'replay' && state.run.status !== 'idle')

  return (
    <footer className="text-[11.5px] text-fg-muted border-t border-line bg-surface-1 flex shrink-0 h-6 items-stretch">
      <DeviceIndicator />
      <div className="px-3 flex flex-1 gap-4 min-w-0 items-center justify-end">
        {replay && (
          <span className="text-accent flex gap-1 items-center">
            <span className="i-ph-clock-counter-clockwise" />
            Offline replay
          </span>
        )}
        {runId && (
          <span className="font-mono flex gap-1 truncate items-center" title="AUV Run of the last execution">
            <span className="i-ph-git-commit shrink-0" />
            {runId}
          </span>
        )}
        {live && (
          <span className="text-good flex gap-1 items-center">
            <span className="rounded-full bg-good size-1.5 animate-pulse" />
            Live
          </span>
        )}
        <span className="flex gap-1 items-center">
          <span className="i-ph-monitor" />
          {`${displays} display${displays === 1 ? '' : 's'}`}
        </span>
      </div>
    </footer>
  )
}
