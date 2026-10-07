/** Labeled form control. */
export function Field({ children, label }: { children: React.ReactNode, label: string }) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-[12px] text-fg-muted font-medium">{label}</span>
      {children}
    </label>
  )
}

/** Secondary explanation under or between form controls. */
export function Hint({ children }: { children: React.ReactNode }) {
  return <span className="text-[11.5px] text-fg-subtle">{children}</span>
}
