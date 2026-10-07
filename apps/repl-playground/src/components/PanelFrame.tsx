/** Workbench panel with a fixed-height title row; `actions` sit on the right of the title. */
export function PanelFrame({ actions, children, title }: { actions?: React.ReactNode, children: React.ReactNode, title: string }) {
  return (
    <section className="bg-surface-1 flex flex-col h-full">
      <div className="pl-3 pr-1.5 border-b border-line flex shrink-0 gap-2 h-9 items-center">
        <span className="panel-title flex-1">{title}</span>
        {actions}
      </div>
      <div className="flex-1 min-h-0 relative">{children}</div>
    </section>
  )
}
