// Prompt.jsx — $ prompt line + a small set of decorated row primitives.

function Blank() { return <div style={{ height: 4 }} /> }

function Comment({ children }) {
  const T = window.AUV_TOKENS
  return <div style={{ color: T.fg3 }}>{children}</div>
}

// "key: value" line with mono alignment via padding (not <table>).
function KV({ indent = 0, k, v, vColor }) {
  const T = window.AUV_TOKENS
  return (
    <div style={{ paddingLeft: indent * 14 }}>
      <span style={{ color: T.fg3 }}>
        {k}
        :
      </span>
      <span style={{ color: vColor || T.fg }}>
        {' '}
        {v}
      </span>
    </div>
  )
}

function Out({ children, color, indent = 0 }) {
  const T = window.AUV_TOKENS
  return (
    <div style={{ color: color || T.fg, paddingLeft: indent * 14 }}>{children}</div>
  )
}

function Prompt({ args, command, cwd = '~/code/auv', host = 'auv', user = 'moeru' }) {
  const T = window.AUV_TOKENS
  return (
    <div style={{ marginTop: 8 }}>
      <span style={{ color: T.validated }}>
        {user}
        @
        {host}
      </span>
      <span style={{ color: T.fg3 }}> </span>
      <span style={{ color: T.brand }}>{cwd}</span>
      <span style={{ color: T.fg3 }}> $ </span>
      <span style={{ color: T.fg }}>{command}</span>
      {args
        ? (
            <span style={{ color: T.fg2 }}>
              {' '}
              {args}
            </span>
          )
        : null}
    </div>
  )
}

// Sigil rows: "● validated   case-id"
function Sigil({ id, indent = 0, kind, label, note }) {
  const T = window.AUV_TOKENS
  const map = {
    boundary: { color: T.boundary, glyph: '○', label: 'not-validated' },
    candidate: { color: T.candidate, glyph: '◐', label: 'candidate' },
    err: { color: T.failed, glyph: '✗', label: 'error' },
    failed: { color: T.failed, glyph: '×', label: 'failed' },
    frozen: { color: T.frozen, glyph: '■', label: 'phase-1-frozen' },
    ok: { color: T.validated, glyph: '✓', label: 'ok' },
    running: { color: T.running, glyph: '●', label: 'running', pulse: true },
    validated: { color: T.validated, glyph: '●', label: 'validated' },
  }
  const m = map[kind] || map.validated
  return (
    <div style={{ paddingLeft: indent * 14 }}>
      <div style={{ alignItems: 'baseline', display: 'flex', gap: 10 }}>
        <span style={{
          animation: m.pulse ? 'auv-pulse 1.2s linear infinite' : 'none',
          color: m.color,
          display: 'inline-block',
          flex: 'none',
          width: 14,
        }}
        >
          {m.glyph}
        </span>
        <span style={{ color: m.color, flex: 'none', width: 100 }}>{label || m.label}</span>
        <span style={{
          color: T.fg,
          flex: '1 1 auto',
          minWidth: 0,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
        >
          {id}
        </span>
      </div>
      {note
        ? (
            <div style={{ color: T.fg3, paddingLeft: 124 }}>
              //
              {' '}
              {note}
            </div>
          )
        : null}
    </div>
  )
}

Object.assign(window, { Blank, Comment, KV, Out, Prompt, Sigil })
