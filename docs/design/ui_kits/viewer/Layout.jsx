// Layout.jsx — Top bar, sidebar shell, content shell.

const VIEWER_T = {
  boundary: '#a73b41',
  boundaryLine: '#ebb1b4',
  boundarySoft: '#f7dcde',
  brand: '#00c4d2',
  brandLine: '#8de1e8',
  brandSoft: '#cff4f7',
  candidate: '#b46a14',
  candidateLine: '#e8c990',
  candidateSoft: '#fbecd3',
  failed: '#c0392b',
  failedLine: '#f0a99e',
  failedSoft: '#fadcd7',
  fg: '#e7e5dd',
  fg2: '#b8b6ad',
  fg3: '#7a7972',
  fg4: '#4f4e49',
  fontMono: '"JetBrains Mono", ui-monospace, "SF Mono", Menlo, Consolas, monospace',
  fontSans: '"Geist", ui-sans-serif, system-ui, sans-serif',
  fontUI: '"Geist Mono", "JetBrains Mono", ui-monospace, Menlo, Consolas, monospace',
  frozen: '#4a5462',
  frozenLine: '#c3cad3',
  frozenSoft: '#e3e7ec',
  ink: '#15171a',
  ink2: '#2c2f34',
  ink3: '#5a5e66',
  ink4: '#8a8e96',
  paper: '#f6f5f1',
  paper2: '#efeee8',
  paper3: '#e6e4dc',
  paperLine: '#d8d5cb',
  running: '#1f7d8c',
  runningLine: '#99ccd3',
  runningSoft: '#d3eaee',
  shell: '#0e1013',
  shell2: '#16181d',
  shell3: '#1e2127',
  shellLine: '#2a2e36',
  validated: '#2f7d4f',
  validatedLine: '#b6d8be',
  validatedSoft: '#dff0e3',
}

function PaneHeader({ dark = true, label, right }) {
  const T = VIEWER_T
  return (
    <div style={{
      alignItems: 'center',
      background: dark ? T.shell2 : T.paper2,
      borderBottom: `1px solid ${dark ? T.shellLine : T.paperLine}`,
      display: 'flex',
      flex: 'none',
      gap: 10,
      height: 32,
      padding: '0 16px',
    }}
    >
      <span style={{
        color: dark ? T.fg3 : T.ink3,
        fontFamily: T.fontUI,
        fontSize: 10,
        fontWeight: 600,
        letterSpacing: 0.8,
        textTransform: 'uppercase',
      }}
      >
        {label}
      </span>
      <div style={{ flex: 1 }} />
      {right}
    </div>
  )
}

function Shell({ children }) {
  return (
    <div style={{
      background: VIEWER_T.shell,
      display: 'flex',
      flexDirection: 'column',
      fontFamily: VIEWER_T.fontSans,
      height: '100vh',
      width: '100%',
    }}
    >
      {children}
    </div>
  )
}

function TopBar({ connection }) {
  const live = connection === 'live'
  const T = VIEWER_T
  return (
    <div style={{
      alignItems: 'center',
      background: T.shell,
      borderBottom: `1px solid ${T.shellLine}`,
      color: T.fg,
      display: 'flex',
      flex: 'none',
      gap: 14,
      height: 44,
      padding: '0 16px',
    }}
    >
      <img alt="" src="../../assets/logo-mark.svg" style={{ height: 22, imageRendering: 'pixelated', width: 22 }} />
      <div style={{ fontFamily: T.fontMono, fontSize: 13, fontWeight: 500 }}>auv</div>
      <div style={{ color: T.fg3, fontFamily: T.fontMono, fontSize: 12 }}>/ inspect viewer</div>
      <img alt="" src="../../assets/sparkle.svg" style={{ height: 14, imageRendering: 'pixelated', opacity: 0.85, width: 14 }} />
      <div style={{ flex: 1 }} />
      <div style={{
        alignItems: 'center',
        background: live ? T.shell2 : T.shell2,
        border: `1px solid ${live ? T.running : T.failed}`,
        borderRadius: 2,
        color: live ? T.running : T.failed,
        display: 'flex',
        fontFamily: T.fontUI,
        fontSize: 12,
        fontWeight: 500,
        gap: 6,
        height: 22,
        padding: '0 9px 0 7px',
      }}
      >
        <span style={{
          animation: live ? 'auv-pulse 1.2s linear infinite' : 'none',
          background: 'currentColor',
          borderRadius: '50%',
          height: 7,
          width: 7,
        }}
        />
        {live ? 'live' : 'disconnected'}
      </div>
      <div style={{ color: T.fg2, fontFamily: T.fontMono, fontSize: 11 }}>
        ws://127.0.0.1:8765/runs/run_1778947574511.../stream
      </div>
    </div>
  )
}

Object.assign(window, { PaneHeader, Shell, TopBar, VIEWER_T })
