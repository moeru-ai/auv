// SpanTree.jsx — indented span tree with status sigils + timing bar.

function depthOf(spans, id, cache = {}) {
  if (id == null)
    return -1
  if (cache[id] != null)
    return cache[id]
  const s = spans.find(x => x.id === id)
  if (!s || s.parent == null)
    return (cache[id] = 0)
  return (cache[id] = depthOf(spans, s.parent, cache) + 1)
}

function spanGlyph(status) {
  const T = VIEWER_T
  if (status === 'running')
    return { c: T.running, g: '●', pulse: true }
  if (status === 'ok')
    return { c: T.validated, g: '●' }
  if (status === 'error')
    return { c: T.failed, g: '×' }
  if (status === 'unset')
    return { c: T.fg3, g: '○' }
  return { c: T.fg3, g: '·' }
}

function SpanTree({ onSelect, selectedSpanId, spans }) {
  const T = VIEWER_T
  // Compute the longest duration for the timing bar normalization (live = use max).
  const totalSecs = spans.reduce((m, s) => {
    const n = Number.parseFloat(s.t)
    return isFinite(n) ? Math.max(m, n) : m
  }, 1)
  return (
    <div style={{ background: T.shell, flex: 1, overflow: 'auto' }}>
      <div style={{
        alignItems: 'center',
        background: T.shell2,
        borderBottom: `1px solid ${T.shellLine}`,
        color: T.fg3,
        display: 'flex',
        fontFamily: T.fontUI,
        fontSize: 10,
        fontWeight: 600,
        gap: 12,
        height: 28,
        letterSpacing: 0.8,
        padding: '0 16px',
        position: 'sticky',
        textTransform: 'uppercase',
        top: 0,
      }}
      >
        <span style={{ width: 14 }} />
        <span style={{ flex: '0 0 300px' }}>span · name / step_id</span>
        <span style={{ flex: '0 0 70px' }}>status</span>
        <span style={{ flex: '0 0 70px' }}>dur</span>
        <span style={{ flex: 1 }}>timing</span>
      </div>
      {spans.map((s) => {
        const d = depthOf(spans, s.id)
        const g = spanGlyph(s.status)
        const selected = s.id === selectedSpanId
        const dur = Number.parseFloat(s.t)
        const pct = isFinite(dur) ? Math.max(2, (dur / totalSecs) * 100) : 0
        // mock bar offsets: cumulative-ish; we just shift later spans visually.
        const offsetPct = Math.min(60, (spans.indexOf(s) * 5))
        return (
          <button
            key={s.id}
            onClick={() => onSelect(s.id)}
            style={{
              alignItems: 'center',
              background: selected ? T.shell3 : 'transparent',
              border: 0,
              borderBottom: `1px solid ${T.shellLine}`,
              borderLeft: `2px solid ${selected ? T.brand : 'transparent'}`,
              color: T.fg,
              cursor: 'pointer',
              display: 'flex',
              fontFamily: T.fontMono,
              fontSize: 12.5,
              gap: 12,
              padding: '7px 16px',
              textAlign: 'left',
              width: '100%',
            }}
          >
            <span style={{ animation: g.pulse ? 'auv-pulse 1.2s linear infinite' : 'none', color: g.c, display: 'inline-block', width: 14 }}>
              {g.g}
            </span>
            <span style={{ color: T.fg, flex: '0 0 300px', overflow: 'hidden', paddingLeft: d * 16, textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              <span style={{ color: T.brandSoft }}>{s.name}</span>
              {s.attrs && s.attrs.step_id
                ? (
                    <span style={{ color: T.fg3 }}>
                      {' '}
                      step_id=
                      {s.attrs.step_id}
                    </span>
                  )
                : null}
              {s.attrs && s.attrs.command_id
                ? (
                    <span style={{ color: T.fg3 }}>
                      {' '}
                      {s.attrs.command_id}
                    </span>
                  )
                : null}
            </span>
            <span style={{ color: g.c, flex: '0 0 70px', fontSize: 11 }}>
              {s.status === 'running' ? 'running' : s.status === 'ok' ? 'ok' : s.status === 'error' ? 'error' : 'unset'}
            </span>
            <span style={{ color: T.fg2, flex: '0 0 70px' }}>{s.t}</span>
            <span style={{ background: T.shell2, borderRadius: 1, flex: 1, height: 8, position: 'relative' }}>
              <span style={{
                background: g.c,
                borderRadius: 1,
                bottom: 0,
                left: `${offsetPct}%`,
                opacity: s.status === 'unset' ? 0.18 : 0.85,
                position: 'absolute',
                top: 0,
                width: `${pct}%`,
              }}
              />
            </span>
          </button>
        )
      })}
    </div>
  )
}

Object.assign(window, { SpanTree })
