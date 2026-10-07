import { Tabs } from '@base-ui/react/tabs'

export interface TabItem {
  /** Badge after the label; hidden when zero or absent. */
  count?: number
  icon?: string
  label: string
  value: string
}

/**
 * Tab list for a `@base-ui` `Tabs.Root`. `header` fills a panel's title row;
 * `segmented` is an inline switch for forms.
 */
export function TabBar({ tabs, variant = 'header' }: { tabs: readonly TabItem[], variant?: 'header' | 'segmented' }) {
  const segmented = variant === 'segmented'
  return (
    <Tabs.List className={segmented ? 'p-0.5 rounded-lg bg-surface-0 flex gap-0.5' : 'px-2 border-b border-line flex shrink-0 gap-1 h-9 items-center'}>
      {tabs.map(tab => (
        <Tabs.Tab
          className={`text-[12.5px] text-fg-muted font-medium px-2.5 rounded-md flex gap-1.5 h-7 cursor-pointer transition-colors items-center justify-center data-[active]:(text-fg bg-surface-2) hover:text-fg ${segmented ? 'flex-1' : ''}`}
          key={tab.value}
          value={tab.value}
        >
          {tab.icon && <span className={`${tab.icon} text-[14px]`} />}
          {tab.label}
          {Boolean(tab.count) && <span className="text-[10.5px] text-fg-subtle font-mono px-1 rounded bg-surface-0">{tab.count}</span>}
        </Tabs.Tab>
      ))}
    </Tabs.List>
  )
}
