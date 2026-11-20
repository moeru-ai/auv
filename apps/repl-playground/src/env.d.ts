/// <reference types="vite/client" />

declare module '*.d.txt?raw' {
  const content: string
  export default content
}

interface ImportMetaEnv {
  /** Registered development client or an HTTPS client metadata address. */
  readonly VITE_EXTERIOR_CLIENT_ID?: string
  /** Exterior relay origin for Agent sign-in; defaults to the local relay. */
  readonly VITE_EXTERIOR_URL?: string
}
