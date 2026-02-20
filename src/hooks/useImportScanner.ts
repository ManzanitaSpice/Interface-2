import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'
import type { DetectedInstance } from '../types/import'

type ScanProgress = {
  stage: string
  message: string
  foundSoFar: number
  currentPath: string
  progressPercent: number
  totalTargets: number
}

export function useImportScanner() {
  const [instances, setInstances] = useState<DetectedInstance[]>([])
  const [status, setStatus] = useState('Ninguna detección activa')
  const [progressPercent, setProgressPercent] = useState(0)
  const [scanLogs, setScanLogs] = useState<string[]>([])
  const [isScanning, setIsScanning] = useState(false)

  useEffect(() => {
    let u1: (() => void) | undefined
    let u2: (() => void) | undefined

    void listen<ScanProgress>('import_scan_progress', (event) => {
      const payload = event.payload
      setStatus(payload.message)
      setProgressPercent(payload.progressPercent ?? 0)
      if (payload.currentPath) {
        const line = `${payload.message} ${payload.currentPath}`
        setScanLogs((prev) => {
          if (prev[prev.length - 1] === line) return prev
          const next = [...prev, line]
          return next.length > 120 ? next.slice(next.length - 120) : next
        })
      }
      if (payload.stage === 'completed') {
        setIsScanning(false)
      }
    }).then((f) => { u1 = f })

    void listen<DetectedInstance>('import_scan_result', (event) => {
      setInstances((prev) => [...prev, event.payload])
    }).then((f) => { u2 = f })

    return () => { u1?.(); u2?.() }
  }, [])

  const scan = async () => {
    setInstances([])
    setStatus('Escaneando...')
    setProgressPercent(0)
    setScanLogs([])
    setIsScanning(true)
    try {
      const found = await invoke<DetectedInstance[]>('detect_external_instances')
      setInstances(found)
      setStatus(`Se encontraron ${found.length} instancias`)
      setProgressPercent(100)
    } finally {
      setIsScanning(false)
    }
  }

  const importSpecific = async (path: string) => {
    const found = await invoke<DetectedInstance[]>('import_specific', { path })
    setInstances((prev) => [...prev, ...found])
  }

  const clear = () => {
    setInstances([])
    setScanLogs([])
    setProgressPercent(0)
    setStatus('Lista vacía')
  }

  return { instances, status, progressPercent, scanLogs, isScanning, scan, importSpecific, clear }
}
