/// <reference types="vite/client" />

declare module '*.d.txt?raw' {
  const content: string
  export default content
}
