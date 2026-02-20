import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'
import type { DetectedInstance } from '../types/import'

type ScanProgress = { stage: string; message: string; foundSoFar: number; currentPath: string }

export function useImportScanner() {
  const [instances, setInstances] = useState<DetectedInstance[]>([])
  const [status, setStatus] = useState('Ninguna detección activa')

  useEffect(() => {
    let u1: (() => void) | undefined
    let u2: (() => void) | undefined

    void listen<ScanProgress>('import_scan_progress', (event) => {
      setStatus(`${event.payload.message} (${event.payload.currentPath})`)
    }).then((f) => { u1 = f })

    void listen<DetectedInstance>('import_scan_result', (event) => {
      setInstances((prev) => [...prev, event.payload])
    }).then((f) => { u2 = f })

    return () => { u1?.(); u2?.() }
  }, [])

  const scan = async () => {
    setInstances([])
    setStatus('Escaneando...')
    const found = await invoke<DetectedInstance[]>('detect_external_instances')
    setInstances(found)
    setStatus(`Se encontraron ${found.length} instancias`) 
  }

  const importSpecific = async (path: string) => {
    const found = await invoke<DetectedInstance[]>('import_specific', { path })
    setInstances((prev) => [...prev, ...found])
  }

  const clear = () => setInstances([])

  return { instances, status, scan, importSpecific, clear }
}
