import { Separator } from 'react-resizable-panels'

export function ResizeHandle({ orientation }: { orientation: 'horizontal' | 'vertical' }) {
  return (
    <Separator
      className={`bg-line transition-colors data-[separator=active]:bg-accent hover:bg-accent ${orientation === 'horizontal' ? 'w-px' : 'h-px'}`}
    />
  )
}
