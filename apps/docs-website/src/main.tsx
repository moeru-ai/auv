import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'

import { CardPage } from './pages/Card'
import { ExportPage } from './pages/Export'
import { FilmPage } from './pages/Film'
import { Landing } from './pages/Landing'
import { MobileLanding } from './pages/MobileLanding'
import { RenderPage } from './pages/Render'
import { useViewport } from './ui/FitStage'

import './styles.css'

// Routes, no router dependency:
//   /        landing (intro film, then the interactive mark over an ambient desk)
//   /film    film player with scrubber, for reviewing timing
//   /render  bare frame-addressable stage for the video exporter
//   /export  how to export the film to video
//   /card    static Open Graph, social preview, and README banner images
const routes: Record<string, () => React.ReactNode> = {
  '/card': () => <CardPage />,
  '/export': () => <ExportPage />,
  '/film': () => <FilmPage />,
  '/render': () => <RenderPage />,
}

/** Portrait and narrow screens get the mobile layout; it re-picks on rotation. */
function ResponsiveLanding() {
  const { h, w } = useViewport()
  const forced = new URLSearchParams(location.search).get('layout')
  const mobile = forced ? forced === 'mobile' : w / h < 0.9 || w < 640
  return mobile ? <MobileLanding /> : <Landing />
}

const page = routes[location.pathname.replace(/\/$/, '')] ?? (() => <ResponsiveLanding />)

// Keep the last render crash inspectable from automation (window.__auvError).
createRoot(document.getElementById('root')!, {
  onUncaughtError: (e) => {
    ;(window as unknown as { __auvError?: string }).__auvError = e instanceof Error ? `${e.message}\n${e.stack}` : String(e)
    console.error(e)
  },
}).render(<StrictMode>{page()}</StrictMode>)
