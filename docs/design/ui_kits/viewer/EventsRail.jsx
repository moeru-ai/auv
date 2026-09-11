// EventsRail.jsx — events.jsonl tail + selected-span detail row above.

function EventsRail({ events, span }) {
  const T = VIEWER_T
  return (
    <div style={{
      background: T.shell2,
      borderTop: `1px solid ${T.shellLine}`,
      display: 'flex',
      flex: '0 0 320px',
      flexDirection: 'column',
    }}
    >
      <SpanDetail span={span} />
      <PaneHeader
        label="Events · events.jsonl"
        right={(
          <span style={{ color: VIEWER_T.fg3, fontFamily: VIEWER_T.fontMono, fontSize: 11 }}>
            {events.length}
            {' '}
            · tail
          </span>
        )}
      />
      <div style={{ flex: 1, overflow: 'auto', padding: '6px 0' }}>
        {events.map((e, i) => (
          <div
            key={i}
            style={{
              background: e.live ? 'rgba(31,125,140,0.08)' : 'transparent',
              display: 'grid',
              fontFamily: T.fontMono,
              fontSize: 12,
              gap: 12,
              gridTemplateColumns: '70px 160px 60px 1fr',
              lineHeight: 1.45,
              padding: '4px 20px',
            }}
          >
            <span style={{ color: T.fg3 }}>{e.t}</span>
            <span style={{ color: e.name.includes('failed') ? T.failed : e.name.includes('started') || e.name.includes('invoke') ? T.brandSoft : T.fg }}>{e.name}</span>
            <span style={{ color: T.fg3 }}>{e.span}</span>
            <span style={{ color: T.fg2 }}>{e.body}</span>
          </div>
        ))}
      </div>
    </div>
  )
}

function SpanDetail({ span }) {
  const T = VIEWER_T
  if (!span) {
    return (
      <div style={{
        alignItems: 'center',
        color: T.fg3,
        display: 'flex',
        fontFamily: T.fontSans,
        fontSize: 13,
        gap: 14,
        padding: '20px',
      }}
      >
        <img alt="" src="../../assets/sparkle.svg" style={{ height: 24, imageRendering: 'pixelated', width: 24 }} />
        Select a span to inspect its attributes.
      </div>
    )
  }
  return (
    <div style={{ color: T.fg, display: 'flex', flexDirection: 'column', gap: 8, padding: '14px 20px' }}>
      <div style={{ alignItems: 'center', display: 'flex', gap: 10 }}>
        <span style={{ color: T.fg3, fontFamily: VIEWER_T.fontUI, fontSize: 10, fontWeight: 600, letterSpacing: 0.8, textTransform: 'uppercase' }}>span</span>
        <span style={{ color: T.brandSoft, fontFamily: T.fontMono, fontSize: 13 }}>{span.name}</span>
        <span style={{ color: T.fg3, fontFamily: T.fontMono, fontSize: 11 }}>
          span_id=
          {span.id}
        </span>
      </div>
      <div style={{
        columnGap: 16,
        display: 'grid',
        fontFamily: T.fontMono,
        fontSize: 12,
        gridTemplateColumns: 'max-content 1fr',
        rowGap: 4,
      }}
      >
        {Object.entries(span.attrs || {}).map(([k, v]) => (
          <React.Fragment key={k}>
            <span style={{ color: T.fg3 }}>{k}</span>
            <span style={{ color: T.fg }}>{String(v)}</span>
          </React.Fragment>
        ))}
      </div>
    </div>
  )
}

Object.assign(window, { EventsRail, SpanDetail })
