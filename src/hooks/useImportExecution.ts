import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'
import type { ImportAction, ImportActionRequest, ImportExecutionProgress, ImportFocusStatus, ImportRequest } from '../types/import'

type ImportExecutionSummary = {
  success: boolean
  action: string
  total: number
  successCount: number
  failureCount: number
  failures: Array<{ instanceId: string; targetName: string; error: string }>
}

export function useImportExecution() {
  const [running, setRunning] = useState(false)
  const [message, setMessage] = useState('')
  const [progressPercent, setProgressPercent] = useState(0)
  const [checkpoints, setCheckpoints] = useState<ImportFocusStatus[]>([])

  useEffect(() => {
    let u1: (() => void) | undefined
    let u2: (() => void) | undefined

    void listen<ImportExecutionProgress>('import_execution_progress', (event) => {
      const payload = event.payload
      const totalSteps = Math.max(payload.total * Math.max(payload.totalSteps, 1), 1)
      const currentStep = (payload.completed * Math.max(payload.totalSteps, 1)) + Math.max(payload.stepIndex, 0)
      setMessage(payload.message)
      setCheckpoints(payload.checkpoints ?? [])
      setProgressPercent(Math.min(100, Math.max(0, Math.round((currentStep / totalSteps) * 100))))
      setRunning(true)
    }).then((f) => { u1 = f })

    void listen<{ success: boolean }>('import_instance_completed', () => {
      setProgressPercent(100)
    }).then((f) => { u2 = f })

    return () => { u1?.(); u2?.() }
  }, [])

  const execute = async (requests: ImportRequest[]) => {
    setRunning(true)
    setProgressPercent(0)
    setCheckpoints([])
    try {
      await invoke('execute_import', { requests })
      setProgressPercent(100)
    } finally {
      setRunning(false)
      setCheckpoints([])
    }
  }

  const executeActionBatch = async (action: ImportAction, requests: ImportActionRequest[]) => {
    if (requests.length === 0) return null

    setRunning(true)
    setProgressPercent(0)
    setCheckpoints([])
    setMessage(`Preparando acción: ${action}`)
    try {
      const summary = await invoke<ImportExecutionSummary>('execute_import_action_batch', { action, requests })
      setProgressPercent(100)
      if (summary.failureCount > 0) {
        const firstError = summary.failures[0]?.error ?? 'Error desconocido'
        setMessage(`Completado con errores (${summary.failureCount}): ${firstError}`)
      } else {
        setMessage(`Completado: ${summary.successCount}/${summary.total}`)
      }
      return summary
    } finally {
      setRunning(false)
      setCheckpoints([])
    }
  }

  return { running, message, progressPercent, checkpoints, execute, executeActionBatch }
}
