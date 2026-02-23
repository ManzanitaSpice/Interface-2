import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { invoke } from '@tauri-apps/api/core'
import './index.css'
import App from './App.tsx'

const isExternalHttpUrl = (url: string) => {
  try {
    const parsed = new URL(url)
    return parsed.protocol === 'http:' || parsed.protocol === 'https:'
  } catch {
    return false
  }
}

document.addEventListener('click', (event) => {
  const target = event.target
  if (!(target instanceof Element)) {
    return
  }

  const anchor = target.closest('a[href]')
  if (!anchor) {
    return
  }

  const href = anchor.getAttribute('href')
  if (!href || !isExternalHttpUrl(href)) {
    return
  }

  event.preventDefault()
  void invoke('open_url_in_browser', { url: href, browserId: 'default' })
})

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
