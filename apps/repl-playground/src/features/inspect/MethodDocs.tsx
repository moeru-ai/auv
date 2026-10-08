import type { MethodDocs as Docs } from '@auv-js/sdk'

import type { CallRecord } from '../../store'

import Markdown from 'react-markdown'

import { useEffect, useState } from 'react'

import { CopyButton } from '../../components/CopyButton'
import { usePlayground } from '../../store'

// Long-form docs per gRPC path, fetched once from the active device. A method
// without docs is remembered; a failed fetch is retried on the next open.
const fetched = new Map<string, Promise<Docs | undefined>>()

/**
 * What an SDK call's method does: its title and description from the schema,
 * and on request its full docs and examples from the Runner. Renders nothing
 * for calls without a presentation (script bindings, unannotated methods).
 */
export function MethodDocs({ call }: { call: CallRecord }) {
  const presentation = call.rpc?.presentation
  const [open, setOpen] = useState(false)
  if (!presentation)
    return null
  return (
    <div className="flex flex-col gap-1">
      <div className="flex gap-2 items-baseline">
        <span className="text-fg font-medium">{presentation.title}</span>
        <button
          className="text-[11.5px] text-fg-subtle ml-auto cursor-pointer hover:text-fg"
          onClick={() => setOpen(!open)}
          type="button"
        >
          {open ? 'Hide docs' : 'Docs'}
        </button>
      </div>
      <p className="text-fg-muted m-0">{presentation.description}</p>
      {open && call.rpc && <FullDocs path={call.rpc.path} />}
    </div>
  )
}

function Examples({ examples }: { examples: Docs['examples'] }) {
  const languages = [...new Set(examples.map(example => example.language))]
  const [picked, setPicked] = useState<string>()
  const language = picked && languages.includes(picked) ? picked : languages[0]
  if (examples.length === 0)
    return null
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex gap-1 items-center">
        <span className="panel-title mr-1">Examples</span>
        {languages.map(candidate => (
          <button
            className={`text-[11px] font-mono px-1.5 rounded cursor-pointer ${candidate === language ? 'text-fg bg-surface-2' : 'text-fg-subtle hover:text-fg'}`}
            key={candidate}
            onClick={() => setPicked(candidate)}
            type="button"
          >
            {candidate}
          </button>
        ))}
      </div>
      {examples.filter(example => example.language === language).map(example => (
        <div className="flex flex-col gap-0.5" key={`${example.title}\n${example.code}`}>
          {example.title && <span className="text-[11.5px] text-fg-muted">{example.title}</span>}
          <div className="rounded bg-surface-2 relative">
            <pre className="text-[11.5px] font-mono m-0 p-2 pr-8 overflow-x-auto">{example.code}</pre>
            <CopyButton className="right-1 top-1 absolute" label="Copy example" text={example.code} />
          </div>
        </div>
      ))}
    </div>
  )
}

function FullDocs({ path }: { path: string }) {
  const [state, setState] = useState<{ docs?: Docs, error?: string, loading: boolean }>({ loading: true })
  useEffect(() => {
    let current = true
    void load(path).then(
      docs => current && setState({ docs, loading: false }),
      (error: unknown) => current && setState({ error: error instanceof Error ? error.message : String(error), loading: false }),
    )
    return () => {
      current = false
    }
  }, [path])
  if (state.loading)
    return <span className="text-[11.5px] text-fg-subtle">Loading docs…</span>
  if (state.error)
    return <span className="text-[11.5px] text-bad">{state.error}</span>
  if (!state.docs)
    return <span className="text-[11.5px] text-fg-subtle">This method has no further docs.</span>
  return (
    <div className="flex flex-col gap-2">
      {/* react-markdown builds React elements and drops raw HTML, so docs from a
          remote Runner cannot inject markup into the page. */}
      <div className="method-docs text-[12px] text-fg-muted max-h-72 overflow-auto">
        <Markdown>{state.docs.markdown}</Markdown>
      </div>
      <Examples examples={state.docs.examples} />
    </div>
  )
}

/** Docs of `path` from the active device; `undefined` when it serves none. */
function load(path: string): Promise<Docs | undefined> {
  let docs = fetched.get(path)
  if (!docs) {
    const sdk = usePlayground.getState().backend?.sdk?.()
    docs = sdk ? sdk.describe(path).then(described => described?.docs()) : Promise.resolve(undefined)
    fetched.set(path, docs)
    // Do not keep a failed fetch: the device may be reachable on the next hover.
    docs.catch(() => fetched.delete(path))
  }
  return docs
}
