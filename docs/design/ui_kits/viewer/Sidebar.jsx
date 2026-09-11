// Sidebar.jsx — run list (left nav, 280px)

function FilterChip({ active, label }) {
  const T = VIEWER_T
  return (
    <span style={{
      background: active ? T.brand : 'transparent',
      border: `1px solid ${active ? T.brand : T.shellLine}`,
      borderRadius: 2,
      color: active ? '#fff' : T.fg2,
      fontFamily: T.fontUI,
      fontSize: 11,
      fontWeight: 500,
      padding: '3px 8px',
    }}
    >
      {label}
    </span>
  )
}

function midTrunc(s, head = 14, tail = 8) {
  if (s.length <= head + tail + 1)
    return s
  return `${s.slice(0, head)}…${s.slice(-tail)}`
}

function Pill({ dark, state, status_code }) {
  const p = statusPill(status_code, state)
  const T = VIEWER_T
  return (
    <span style={{
      alignItems: 'center',
      background: dark ? 'transparent' : p.bg,
      border: `1px solid ${dark ? p.color : p.line}`,
      borderRadius: 2,
      color: p.color,
      display: 'inline-flex',
      fontFamily: T.fontUI,
      fontSize: 11,
      fontWeight: 500,
      gap: 6,
      height: 20,
      padding: '0 8px 0 6px',
    }}
    >
      <span style={{
        animation: p.pulse ? 'auv-pulse 1.2s linear infinite' : 'none',
        background: 'currentColor',
        borderRadius: '50%',
        height: 7,
        width: 7,
      }}
      />
      {p.label}
    </span>
  )
}

function RunTypeChip({ run_type }) {
  const T = VIEWER_T
  return (
    <span style={{
      border: `1px solid ${T.shellLine}`,
      borderRadius: 2,
      color: T.fg3,
      fontFamily: T.fontUI,
      fontSize: 10,
      fontWeight: 500,
      letterSpacing: 0.4,
      padding: '1px 6px',
    }}
    >
      {run_type}
    </span>
  )
}

function Sidebar({ activeId, onSelect, runs }) {
  const T = VIEWER_T
  return (
    <div style={{
      background: T.shell2,
      borderRight: `1px solid ${T.shellLine}`,
      display: 'flex',
      flex: 'none',
      flexDirection: 'column',
      overflow: 'hidden',
      width: 320,
    }}
    >
      <PaneHeader
        label="Runs · /runs"
        right={
          <span style={{ color: T.fg3, fontFamily: T.fontMono, fontSize: 11 }}>{runs.length}</span>
        }
      />
      <div style={{ borderBottom: `1px solid ${T.shellLine}`, display: 'flex', flexWrap: 'wrap', gap: 6, padding: '8px 12px' }}>
        <FilterChip active label="all" />
        <FilterChip label="running" />
        <FilterChip label="error" />
        <FilterChip label="execute" />
        <FilterChip label="validate" />
        <FilterChip label="probe" />
      </div>
      <div style={{ flex: 1, overflow: 'auto' }}>
        {runs.map((r) => {
          const active = r.run_id === activeId
          return (
            <button
              key={r.run_id}
              onClick={() => onSelect(r.run_id)}
              style={{
                background: active ? T.shell3 : 'transparent',
                border: 0,
                borderBottom: `1px solid ${T.shellLine}`,
                borderLeft: `2px solid ${active ? T.brand : 'transparent'}`,
                color: T.fg,
                cursor: 'pointer',
                display: 'flex',
                flexDirection: 'column',
                gap: 6,
                padding: '12px 14px',
                textAlign: 'left',
                width: '100%',
              }}
            >
              <div style={{ alignItems: 'center', display: 'flex', gap: 8 }}>
                <Pill dark state={r.state} status_code={r.status_code} />
                <RunTypeChip run_type={r.run_type} />
                <span style={{ flex: 1 }} />
                <span style={{ color: T.fg3, fontFamily: T.fontMono, fontSize: 11 }}>{r.duration}</span>
              </div>
              <div style={{ color: T.fg, fontFamily: T.fontMono, fontSize: 12, lineHeight: 1.35 }}>
                {midTrunc(r.run_id, 22, 8)}
              </div>
              <div style={{ color: T.fg2, fontFamily: T.fontSans, fontSize: 12, lineHeight: 1.4 }}>
                {r.summary}
              </div>
              <div style={{ color: T.fg3, display: 'flex', fontFamily: T.fontMono, fontSize: 10, gap: 12 }}>
                <span>
                  {r.spans}
                  {' '}
                  spans
                </span>
                <span>
                  {r.artifacts}
                  {' '}
                  artifacts
                </span>
              </div>
            </button>
          )
        })}
      </div>
    </div>
  )
}

function statusPill(status_code, state) {
  const T = VIEWER_T
  if (state === 'running')
    return { bg: T.runningSoft, color: T.running, label: 'running', line: T.runningLine, pulse: true }
  if (status_code === 'ok')
    return { bg: T.validatedSoft, color: T.validated, label: 'ok', line: T.validatedLine }
  if (status_code === 'error')
    return { bg: T.failedSoft, color: T.failed, label: 'error', line: T.failedLine }
  return { bg: T.frozenSoft, color: T.frozen, label: 'unset', line: T.frozenLine }
}

Object.assign(window, { Pill, Sidebar })
