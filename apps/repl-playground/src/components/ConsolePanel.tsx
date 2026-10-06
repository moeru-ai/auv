import { useEffect, useRef } from 'react'

import { actions, usePlayground } from '../store'
import { Empty } from './Timeline'
import { ValueView } from './ValueView'

const LEVEL_STYLE = {
  error: 'text-bad bg-bad/6',
  info: 'text-sky-300',
  log: 'text-fg',
  show: 'text-violet-300 bg-violet-500/6',
  warn: 'text-warn bg-warn/6',
} as const

export function ConsolePanel() {
  const logs = usePlayground(state => state.logs)
  const endRef = useRef<HTMLDivElement>(null)

  const stickRef = useRef(true)

  // Stick to the newest entry unless the user scrolled up to read.
  useEffect(() => {
    if (stickRef.current)
      endRef.current?.scrollIntoView({ block: 'end' })
  }, [logs.length])

  if (logs.length === 0)
    return <Empty>console.log(), show() and cell results appear here.</Empty>

  return (
    <div
      className="text-[12.5px] font-mono h-full relative overflow-auto"
      onScroll={(event) => {
        const element = event.currentTarget
        stickRef.current = element.scrollHeight - element.scrollTop - element.clientHeight < 24
      }}
    >
      <button className="btn-ghost right-2 top-1 absolute z-1" onClick={actions.clearConsole} type="button">
        <span className="i-lucide-trash-2" />
        Clear
      </button>
      {logs.map(entry => (
        <div className={`px-3 py-1.5 border-b border-line/50 flex gap-3 ${LEVEL_STYLE[entry.level]}`} key={entry.id}>
          <span className="text-fg-subtle shrink-0 w-10">{entry.line === null ? '' : `L${entry.line}`}</span>
          {entry.label && <span className="text-fg-subtle shrink-0">{`${entry.label}:`}</span>}
          <div className="flex flex-wrap gap-x-3 min-w-0">
            {entry.values.map((value, index) => (
              <span className="min-w-0 break-all" key={index}>
                {typeof value === 'string' && entry.level !== 'show' ? value : <ValueView value={value} />}
              </span>
            ))}
          </div>
        </div>
      ))}
      <div ref={endRef} />
    </div>
  )
}
