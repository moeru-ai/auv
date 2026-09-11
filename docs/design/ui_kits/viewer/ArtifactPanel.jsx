// ArtifactPanel.jsx — right rail showing artifact list + selected preview.

function ArtifactIcon({ mime }) {
  const isImg = mime && mime.startsWith('image/')
  const isJson = mime === 'application/json'
  const src = isImg
    ? '../../assets/icon-png.svg'
    : isJson
      ? '../../assets/icon-json.svg'
      : '../../assets/icon-bin.svg'
  return (
    <img
      alt=""
      src={src}
      style={{
        display: 'block',
        flex: 'none',
        height: 28,
        imageRendering: 'pixelated',
        width: 28,
      }}
    />
  )
}

function ArtifactPanel({ artifacts, onSelect, selectedId }) {
  const T = VIEWER_T
  const selected = artifacts.find(a => a.id === selectedId)
  return (
    <div style={{
      background: T.shell2,
      borderLeft: `1px solid ${T.shellLine}`,
      display: 'flex',
      flex: 'none',
      flexDirection: 'column',
      overflow: 'hidden',
      width: 340,
    }}
    >
      <PaneHeader
        label="Artifacts · /artifacts"
        right={
          <span style={{ color: T.fg3, fontFamily: T.fontMono, fontSize: 11 }}>{artifacts.length}</span>
        }
      />
      <div style={{ borderBottom: `1px solid ${T.shellLine}` }}>
        {artifacts.map((a) => {
          const isActive = a.id === selectedId
          return (
            <button
              key={a.id}
              onClick={() => onSelect(a.id)}
              style={{
                alignItems: 'center',
                background: isActive ? T.shell3 : 'transparent',
                border: 0,
                borderLeft: `2px solid ${isActive ? T.brand : 'transparent'}`,
                color: T.fg,
                cursor: 'pointer',
                display: 'flex',
                gap: 10,
                padding: '10px 12px',
                textAlign: 'left',
                width: '100%',
              }}
            >
              <ArtifactIcon mime={a.mime} />
              <div style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0 }}>
                <span style={{ color: T.fg, fontFamily: T.fontMono, fontSize: 11.5, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {a.role}
                </span>
                <span style={{ color: T.fg3, fontFamily: T.fontMono, fontSize: 10.5 }}>
                  {a.path.split('/').pop()}
                </span>
              </div>
              <div style={{ flex: 1 }} />
              {a.live
                ? (
                    <span style={{
                      border: `1px solid ${T.running}`,
                      borderRadius: 2,
                      color: T.running,
                      fontFamily: T.fontUI,
                      fontSize: 9.5,
                      fontWeight: 500,
                      padding: '1px 6px',
                    }}
                    >
                      live
                    </span>
                  )
                : null}
            </button>
          )
        })}
      </div>
      <div style={{ flex: 1, overflow: 'auto' }}>
        <ArtifactPreview a={selected} />
      </div>
    </div>
  )
}

function ArtifactPreview({ a }) {
  const T = VIEWER_T
  if (!a) {
    return (
      <div style={{
        alignItems: 'center',
        color: T.fg3,
        display: 'flex',
        flexDirection: 'column',
        fontFamily: T.fontSans,
        fontSize: 12,
        gap: 12,
        padding: '30px 16px 24px',
        textAlign: 'center',
      }}
      >
        <img
          alt=""
          src="../../assets/sprite-inspector.svg"
          style={{ height: 112, imageRendering: 'pixelated', width: 96 }}
        />
        <div style={{ color: T.fg2 }}>Select an artifact to preview.</div>
        <div style={{ color: T.fg4, fontFamily: T.fontMono, fontSize: 10.5 }}>3 artifacts on this run</div>
      </div>
    )
  }
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 10, padding: '14px 16px' }}>
      <div style={{
        display: 'grid',
        fontFamily: T.fontMono,
        fontSize: 11.5,
        gap: '4px 14px',
        gridTemplateColumns: 'max-content 1fr',
      }}
      >
        <span style={{ color: T.fg3 }}>role</span>
        {' '}
        <span style={{ color: T.fg }}>{a.role}</span>
        <span style={{ color: T.fg3 }}>mime</span>
        {' '}
        <span style={{ color: T.fg }}>{a.mime}</span>
        <span style={{ color: T.fg3 }}>path</span>
        {' '}
        <span style={{ color: T.fg }}>{a.path}</span>
        <span style={{ color: T.fg3 }}>sha256</span>
        {' '}
        <span style={{ color: T.fg }}>{a.sha}</span>
        <span style={{ color: T.fg3 }}>bytes</span>
        {' '}
        <span style={{ color: T.fg }}>{a.bytes}</span>
        <span style={{ color: T.fg3 }}>span_id</span>
        {' '}
        <span style={{ color: T.fg }}>{a.span}</span>
      </div>
      {/* Stand-in preview surface */}
      <div style={{
        background: a.mime === 'application/json'
          ? T.shell3
          : `repeating-linear-gradient(45deg, ${T.shell2} 0 12px, ${T.shell3} 12px 24px)`,
        border: `1px solid ${T.shellLine}`,
        borderRadius: 4,
        height: 220,
        marginTop: 6,
        overflow: 'hidden',
        position: 'relative',
      }}
      >
        {a.mime === 'application/json'
          ? (
              <pre style={{
                color: T.fg2,
                fontFamily: T.fontMono,
                fontSize: 11.5,
                lineHeight: 1.5,
                margin: 0,
                padding: 14,
                whiteSpace: 'pre-wrap',
              }}
              >
                {`{
  "api_version": "auv.artifact.v1alpha1",
  "role": "ax.before",
  "subjectBundleId": "com.tencent.QQMusicMac",
  "windowRef": "win:0x83a1",
  "rootRole": "AXApplication",
  "childCount": 412,
  "notes": [
    "captured before resolve-ocr-anchor",
    "ax tree subset; full payload in artifacts/"
  ]
}`}
              </pre>
            )
          : (
              <div style={{
                alignItems: 'center',
                color: T.fg2,
                display: 'flex',
                fontFamily: T.fontUI,
                fontSize: 11,
                inset: 0,
                justifyContent: 'center',
                letterSpacing: 0.4,
                position: 'absolute',
              }}
              >
                screenshot ·
                {' '}
                {a.bytes}
              </div>
            )}
      </div>
    </div>
  )
}

Object.assign(window, { ArtifactPanel })
