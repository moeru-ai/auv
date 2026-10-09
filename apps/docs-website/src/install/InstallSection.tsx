// "Install" section shared by the desktop and mobile landings: pick a system,
// then a package manager (and a CPU where the download differs), and copy the
// commands from a code block. Defaults to the visitor's own system.
//
// The AUV mark sits on top. Now and then its A or V turns into a ghost cursor
// and clicks through the install methods (see GhostDemo.tsx).

import type { Theme } from '../theme'
import type { Method, Os, Shell } from './methods'

import { Tabs } from '@base-ui/react/tabs'
import { useEffect, useMemo, useRef, useState } from 'react'

import { MONO } from '../render/WindowContent'
import { Icon } from '../ui/Chrome'
import { GhostDemo } from './GhostDemo'
import { codeOf, detectArch, detectSystem, SYSTEMS } from './methods'

export const INSTALL_ID = 'install'
/** The mark's slot; the desktop landing flies its hero mark into it. */
export const INSTALL_MARK_ID = 'install-mark'

interface Props {
  bottomPad?: number
  compact?: boolean
  /** Hide the section's own mark while another copy is flying in. */
  markHidden?: boolean
  theme: Theme
}

export function CodeBlock({ code, shell }: { code: string, shell: Shell }) {
  const [copied, setCopied] = useState(false)
  const timer = useRef(0)
  useEffect(() => () => clearTimeout(timer.current), [])
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(code)
    }
    catch {
      // Insecure origins (plain http on a LAN IP) have no async clipboard.
      const ta = document.createElement('textarea')
      ta.value = code
      ta.style.cssText = 'position:fixed;opacity:0'
      document.body.appendChild(ta)
      ta.select()
      document.execCommand('copy')
      ta.remove()
    }
    setCopied(true)
    clearTimeout(timer.current)
    timer.current = window.setTimeout(setCopied, 1600, false)
  }
  const prompt = shell === 'powershell' ? '>' : '$'
  return (
    <div className="code">
      <div className="code-head">
        <span>{shell === 'powershell' ? 'PowerShell' : 'Terminal'}</span>
        <button aria-label="Copy commands" className={copied ? 'done' : ''} onClick={copy}>
          <Icon name={copied ? 'check' : 'copy'} size={15} />
          <span>{copied ? 'Copied' : 'Copy'}</span>
        </button>
      </div>
      <pre style={{ fontFamily: MONO }}>
        {code.split('\n').map((line, i) => {
          const comment = line.startsWith('#')
          const [cmd, ...rest] = line.split(' ')
          return (
            <div className={comment ? 'code-line comment' : 'code-line'} key={i}>
              <span aria-hidden="true" className="code-prompt">{comment ? ' ' : prompt}</span>
              {comment
                ? <span>{line}</span>
                : (
                    <span>
                      <span className="code-cmd">{cmd}</span>
                      {rest.length > 0 && ` ${rest.join(' ')}`}
                    </span>
                  )}
            </div>
          )
        })}
      </pre>
    </div>
  )
}

export function InstallSection({ bottomPad = 0, compact = false, markHidden = false, theme }: Props) {
  const detected = useMemo(detectSystem, [])
  const [osId, setOsId] = useState(detected.os)
  const os = SYSTEMS.find(o => o.id === osId)!
  // Remember the picked method per system, so switching systems and back keeps it.
  const [picked, setPicked] = useState<Record<string, string>>({})
  const method = os.methods.find(m => m.id === picked[os.id]) ?? os.methods[0]
  const pick = (id: string) => setPicked(p => ({ ...p, [os.id]: id }))
  const [arch, setArch] = useState(detected.arch)
  // Until the visitor touches the section, the ghosts may switch methods for them.
  const [touched, setTouched] = useState(false)
  const archPickedRef = useRef(false)
  useEffect(() => {
    // The client-hint CPU arrives late; it never overrides the visitor's own pick.
    void detectArch().then(a => a && !archPickedRef.current && setArch(a))
  }, [])
  const section = useRef<HTMLElement>(null)
  const mark = useRef<HTMLDivElement>(null)

  return (
    <section
      className={`install ${compact ? 'compact' : ''}`}
      id={INSTALL_ID}
      onKeyDown={() => setTouched(true)}
      onPointerDown={() => setTouched(true)}
      ref={section}
      style={{ paddingBottom: bottomPad + (compact ? 96 : 120) }}
    >
      <div className="install-inner">
        <div className="install-mark" id={INSTALL_MARK_ID} ref={mark} />
        <p className="install-kicker">Install</p>
        <h2>Get AUV on every desk.</h2>
        <p className="install-lede">One CLI for macOS, Linux, and Windows. Pick your system and package manager, then copy the commands.</p>

        <Tabs.Root onValueChange={v => setOsId(v as Os['id'])} value={os.id}>
          <Tabs.List aria-label="System" className="seg install-os">
            {SYSTEMS.map(o => (
              <Tabs.Tab className="seg-tab" key={o.id} value={o.id}>
                <Icon name={o.icon} size={compact ? 20 : 22} />
                <span>{o.label}</span>
              </Tabs.Tab>
            ))}
            <Tabs.Indicator className="seg-ind" />
          </Tabs.List>
        </Tabs.Root>

        <div className="install-row">
          <Tabs.Root onValueChange={v => pick(v as string)} value={method.id}>
            <Tabs.List aria-label="Install method" className="seg install-methods">
              {os.methods.map(m => (
                <Tabs.Tab className="seg-tab" data-ghost-method={m.id} key={m.id} value={m.id}>
                  <Icon name={m.icon} size={16} />
                  <span>{m.label}</span>
                </Tabs.Tab>
              ))}
              <Tabs.Indicator className="seg-ind" />
            </Tabs.List>
          </Tabs.Root>
          {method.archs && (
            <Tabs.Root
              onValueChange={(v) => {
                archPickedRef.current = true
                setArch(v as string)
              }}
              value={arch}
            >
              <Tabs.List aria-label="CPU" className="seg install-arch">
                {method.archs.map(a => <Tabs.Tab className="seg-tab" key={a.id} value={a.id}>{a.label}</Tabs.Tab>)}
                <Tabs.Indicator className="seg-ind" />
              </Tabs.List>
            </Tabs.Root>
          )}
        </div>

        <MethodBlock arch={arch} key={`${os.id}-${method.id}`} method={method} os={os} />
      </div>

      <GhostDemo
        hidden={markHidden}
        mark={mark}
        pickTarget={() => {
          // Before the visitor picks anything, try the next method; afterwards
          // only double-check their pick, so their choice never changes.
          const i = os.methods.indexOf(method)
          const m = touched ? method : os.methods[(i + 1) % os.methods.length]
          const el = section.current?.querySelector<HTMLElement>(`[data-ghost-method="${m.id}"]`)
          return el ? { el, label: touched ? `checking “${m.label}”` : `trying “${m.label}” for you`, onClick: touched ? undefined : () => pick(m.id) } : null
        }}
        section={section}
        theme={theme}
      />
    </section>
  )
}

function MethodBlock({ arch, method, os }: { arch: string, method: Method, os: Os }) {
  return (
    <div className="install-method">
      <CodeBlock code={codeOf(method, arch)} shell={os.shell} />
      {method.note && <p className="install-note">{method.note}</p>}
      {os.setup && method.helper && (
        <>
          <h3>{os.setup.title}</h3>
          <p className="install-about">
            <Icon name="info" size={18} />
            <span>{os.setup.about}</span>
          </p>
          <CodeBlock code={os.setup.code} shell={os.shell} />
          <p className="install-note">{os.setup.note}</p>
        </>
      )}
    </div>
  )
}
