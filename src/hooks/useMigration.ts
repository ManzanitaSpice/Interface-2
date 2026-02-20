import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'

export type MigrationProgress = { step: string; completed: number; total: number; message: string }

export function useMigration() {
  const [progress, setProgress] = useState<MigrationProgress | null>(null)
  const [isMigrating, setIsMigrating] = useState(false)

  useEffect(() => {
    let unlisten: (() => void) | undefined
    void listen<MigrationProgress>('migration_progress', (event) => {
      setProgress(event.payload)
      setIsMigrating(true)
      if (event.payload.completed >= event.payload.total) {
        setTimeout(() => {
          setIsMigrating(false)
          setProgress(null)
        }, 600)
      }
    }).then((fn) => { unlisten = fn })

    return () => { unlisten?.() }
  }, [])

  const migrateLauncherRoot = async (newPath: string, migrateFiles: boolean) => {
    setIsMigrating(true)
    await invoke('migrate_launcher_root', { newPath, migrateFiles })
    setIsMigrating(false)
  }

  const changeInstancesFolder = async (newPath: string, migrateFiles: boolean) => {
    setIsMigrating(true)
    await invoke('change_instances_folder', { newPath, migrateFiles })
    setIsMigrating(false)
  }

  return { progress, isMigrating, migrateLauncherRoot, changeInstancesFolder }
}
