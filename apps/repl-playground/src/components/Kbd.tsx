export function Kbd({ children, className = '' }: { children: React.ReactNode, className?: string }) {
  return <kbd className={`text-[11px] text-fg-muted font-mono px-1 border border-line rounded bg-surface-2 ${className}`}>{children}</kbd>
}
