// Terminal.jsx — macOS Terminal.app-style window chrome + scrollback container
// Exports: Terminal, TabStrip

function TabStrip({ active, onSelect, tabs }) {
  const T = window.AUV_TOKENS
  return (
    <div style={{
      alignItems: 'stretch',
      background: '#1f2228',
      borderBottom: '1px solid #000',
      display: 'flex',
      height: 30,
    }}
    >
      {tabs.map((tab, i) => {
        const isActive = i === active
        return (
          <button
            key={tab.id}
            onClick={() => onSelect(i)}
            style={{
              background: isActive ? T.shell : 'transparent',
              border: 0,
              borderRight: i < tabs.length - 1 ? '1px solid #000' : 0,
              borderTop: isActive ? `1px solid ${T.brand}` : '1px solid transparent',
              color: isActive ? T.fg : T.fg3,
              cursor: 'pointer',
              flex: 1,
              fontFamily: T.fontMono,
              fontSize: 11,
              fontWeight: 500,
              letterSpacing: 0.2,
              overflow: 'hidden',
              padding: '0 14px',
              textAlign: 'left',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
            title={tab.label}
          >
            {tab.label}
          </button>
        )
      })}
    </div>
  )
}

function Terminal({ active, children, height = 540, onSelect, tabs, title = 'auv — bash — 132×40' }) {
  const T = window.AUV_TOKENS
  return (
    <div style={{
      background: T.shell,
      border: '1px solid #000',
      borderRadius: 10,
      boxShadow: '0 24px 60px rgba(0,0,0,0.35), 0 6px 16px rgba(0,0,0,0.2)',
      margin: '0 auto',
      maxWidth: 1000,
      overflow: 'hidden',
      width: '100%',
    }}
    >
      {/* titlebar */}
      <div style={{
        alignItems: 'center',
        background: 'linear-gradient(#3a3d44, #2c2f35)',
        borderBottom: '1px solid #000',
        display: 'flex',
        height: 28,
        padding: '0 12px',
        position: 'relative',
      }}
      >
        <TrafficLights />
        <div style={{
          alignItems: 'center',
          color: '#d6d6d6',
          display: 'flex',
          fontFamily: T.fontSans,
          fontSize: 12,
          fontWeight: 500,
          inset: 0,
          justifyContent: 'center',
          pointerEvents: 'none',
          position: 'absolute',
        }}
        >
          {title}
        </div>
      </div>
      {/* tabs */}
      {tabs && tabs.length > 1
        ? (
            <TabStrip active={active} onSelect={onSelect} tabs={tabs} />
          )
        : null}
      {/* scrollback */}
      <div style={{
        background: T.shell,
        color: T.fg,
        fontFamily: T.fontMono,
        fontSize: 12.5,
        height,
        lineHeight: 1.6,
        overflow: 'auto',
        padding: '16px 20px 24px',
      }}
      >
        {children}
      </div>
    </div>
  )
}

function TrafficLights() {
  const dot = bg => (
    <span style={{
      background: bg,
      borderRadius: '50%',
      boxShadow: 'inset 0 0 0 0.5px rgba(0,0,0,0.25)',
      display: 'inline-block',
      height: 12,
      width: 12,
    }}
    />
  )
  return (
    <div style={{ alignItems: 'center', display: 'flex', gap: 8, padding: '0 0 0 4px' }}>
      {dot('#ff5f57')}
      {dot('#febc2e')}
      {dot('#28c840')}
    </div>
  )
}

Object.assign(window, { TabStrip, Terminal })
