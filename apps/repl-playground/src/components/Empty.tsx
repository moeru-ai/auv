/** Centered placeholder for a panel with nothing to show yet. */
export function Empty({ children }: { children: React.ReactNode }) {
  return <div className="text-fg-subtle p-6 text-center flex h-full items-center justify-center">{children}</div>
}
