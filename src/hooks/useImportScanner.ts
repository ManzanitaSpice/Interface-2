import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useRef, useState } from 'react'
import type { DetectedInstance } from '../types/import'

type ScanProgress = {
  stage: string
  message: string
  foundSoFar: number
  currentPath: string
  progressPercent: number
  totalTargets: number
}

const importScannerStorageKey = 'launcher_import_scanner_v1'

const dedupeInstances = (items: DetectedInstance[]) => {
  const byPath = new Set<string>()
  const out: DetectedInstance[] = []

  for (const item of items) {
    const key = item.sourcePath.trim().toLowerCase()
    if (!key || byPath.has(key)) continue
    byPath.add(key)
    out.push(item)
  }

  return out
}

export function useImportScanner() {
  const [instances, setInstances] = useState<DetectedInstance[]>([])
  const [status, setStatus] = useState('Ninguna detección activa')
  const [progressPercent, setProgressPercent] = useState(0)
  const [scanLogs, setScanLogs] = useState<string[]>([])
  const [isScanning, setIsScanning] = useState(false)
  const [keepDetected, setKeepDetected] = useState(true)
  const lastProgressLogAtRef = useRef(0)

  useEffect(() => {
    try {
      const raw = localStorage.getItem(importScannerStorageKey)
      if (!raw) return
      const parsed = JSON.parse(raw) as { instances?: DetectedInstance[]; keepDetected?: boolean }
      setInstances(Array.isArray(parsed.instances) ? dedupeInstances(parsed.instances) : [])
      setKeepDetected(parsed.keepDetected !== false)
      if ((parsed.instances?.length ?? 0) > 0) {
        setStatus(`Instancias recuperadas: ${parsed.instances?.length ?? 0}`)
      }
    } catch {
      localStorage.removeItem(importScannerStorageKey)
    }
  }, [])

  useEffect(() => {
    localStorage.setItem(importScannerStorageKey, JSON.stringify({
      instances: keepDetected ? instances : [],
      keepDetected,
    }))
  }, [instances, keepDetected])

  useEffect(() => {
    let u1: (() => void) | undefined
    let u2: (() => void) | undefined

    void listen<ScanProgress>('import_scan_progress', (event) => {
      const payload = event.payload
      setStatus((prev) => (prev === payload.message ? prev : payload.message))
      setProgressPercent((prev) => (prev === (payload.progressPercent ?? 0) ? prev : (payload.progressPercent ?? 0)))
      if (payload.currentPath) {
        const now = Date.now()
        if (now - lastProgressLogAtRef.current < 120) return
        lastProgressLogAtRef.current = now
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
      setInstances((prev) => dedupeInstances([...prev, event.payload]))
    }).then((f) => { u2 = f })

    return () => { u1?.(); u2?.() }
  }, [])

  const scan = async () => {
    if (!keepDetected) {
      setInstances([])
    }
    setStatus('Escaneando...')
    setProgressPercent(0)
    setScanLogs([])
    setIsScanning(true)
    try {
      const found = await invoke<DetectedInstance[]>('detect_external_instances')
      const uniqueFound = dedupeInstances(keepDetected ? [...instances, ...found] : found)
      setInstances(uniqueFound)
      setStatus(`Se encontraron ${uniqueFound.length} instancias`)
      setProgressPercent(100)
    } finally {
      setIsScanning(false)
    }
  }

  const importSpecific = async (path: string) => {
    const found = await invoke<DetectedInstance[]>('import_specific', { path })
    setInstances((prev) => dedupeInstances([...prev, ...found]))
  }

  const clear = () => {
    setInstances([])
    setScanLogs([])
    setProgressPercent(0)
    setStatus('Lista vacía')
  }

  return {
    instances,
    status,
    progressPercent,
    scanLogs,
    isScanning,
    keepDetected,
    setKeepDetected,
    scan,
    importSpecific,
    clear,
  }
}
