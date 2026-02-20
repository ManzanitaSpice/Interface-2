import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'
import type { ImportRequest } from '../types/import'

export function useImportExecution() {
  const [running, setRunning] = useState(false)
  const [message, setMessage] = useState('')

  useEffect(() => {
    let u1: (() => void) | undefined
    let u2: (() => void) | undefined

    void listen<{ message: string }>('import_execution_progress', (event) => {
      setMessage(event.payload.message)
      setRunning(true)
    }).then((f) => { u1 = f })

    void listen<{ success: boolean }>('import_instance_completed', () => {
      setRunning(false)
    }).then((f) => { u2 = f })

    return () => { u1?.(); u2?.() }
  }, [])

  const execute = async (requests: ImportRequest[]) => {
    setRunning(true)
    await invoke('execute_import', { requests })
    setRunning(false)
  }

  return { running, message, execute }
}
