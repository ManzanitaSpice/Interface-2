import { invoke } from '@tauri-apps/api/core'
import { useMemo, useState } from 'react'
import { DetectedInstanceCard } from '../components/import/DetectedInstanceCard'
import { ImportProgressModal } from '../components/import/ImportProgressModal'
import { ImportSidePanel } from '../components/import/ImportSidePanel'
import { ImportToolbar } from '../components/import/ImportToolbar'
import { ScanStatusBar } from '../components/import/ScanStatusBar'
import { useImportExecution } from '../hooks/useImportExecution'
import { useImportScanner } from '../hooks/useImportScanner'
import type { ImportAction, ImportActionRequest, ImportRequest } from '../types/import'

type Props = {
  onInstancesChanged?: () => Promise<void> | void
}

export function ImportPage({ onInstancesChanged }: Props) {
  const { instances, status, progressPercent, scanLogs, isScanning, keepDetected, setKeepDetected, scan, clear } = useImportScanner()
  const { running, message, progressPercent: executionProgressPercent, execute, executeActionBatch } = useImportExecution()
  const [selected, setSelected] = useState<string[]>([])
  const [search, setSearch] = useState('')

  const selectedItems = useMemo(() => instances.filter((item) => selected.includes(item.id)), [instances, selected])
  const filteredInstances = useMemo(() => {
    const query = search.trim().toLowerCase()
    if (!query) return instances
    return instances.filter((item) => item.name.toLowerCase().includes(query) || item.sourceLauncher.toLowerCase().includes(query))
  }, [instances, search])

  const buildImportRequests = (items = selectedItems): ImportRequest[] => items.filter((item) => item.importable).map((item) => ({
    detectedInstanceId: item.id,
    sourcePath: item.sourcePath,
    targetName: item.name,
    targetGroup: 'Importadas',
    minecraftVersion: item.minecraftVersion,
    loader: item.loader,
    loaderVersion: item.loaderVersion,
    ramMb: 4096,
    copyMods: true,
    copyWorlds: true,
    copyResourcepacks: true,
    copyScreenshots: false,
    copyLogs: false,
  }))

  const buildActionRequests = (action: ImportAction): ImportActionRequest[] => selectedItems
    .filter((entry) => entry.importable)
    .map((item) => ({
      detectedInstanceId: item.id,
      sourcePath: item.sourcePath,
      targetName: action === 'clonar' ? `${item.name}-copia` : item.name,
      targetGroup: action === 'migrar' ? 'Migradas' : 'Importadas',
      minecraftVersion: item.minecraftVersion,
      loader: item.loader,
      loaderVersion: item.loaderVersion,
      action,
    }))

  const runImport = async (requests: ImportRequest[]) => {
    if (requests.length === 0) return
    await execute(requests)
    await onInstancesChanged?.()
  }

  const executeAction = async (action: ImportAction) => {
    const requests = buildActionRequests(action)
    if (requests.length === 0) return
    await executeActionBatch(action, requests)
    await onInstancesChanged?.()
  }

  const openSelectedFolder = async () => {
    const first = selectedItems[0]
    if (!first) return
    await invoke('execute_import_action', {
      request: {
        detectedInstanceId: first.id,
        sourcePath: first.sourcePath,
        targetName: first.name,
        targetGroup: 'Importadas',
        minecraftVersion: first.minecraftVersion,
        loader: first.loader,
        loaderVersion: first.loaderVersion,
        action: 'abrir_carpeta',
      },
    })
  }

  return (
    <main className="content content-padded">
      <section className="instances-panel huge-panel import-page">
        <ImportToolbar
          status={status}
          detectedCount={instances.length}
          selectedCount={selectedItems.length}
          isScanning={isScanning}
          keepDetected={keepDetected}
          onToggleKeepDetected={() => setKeepDetected((prev) => !prev)}
          onScan={() => void scan()}
          onClear={() => { clear(); setSelected([]) }}
        />
        <ScanStatusBar status={status} progressPercent={progressPercent} scanLogs={scanLogs} isScanning={isScanning} />
        <div className="instance-search-row">
          <input
            className="instance-search-compact"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder="Buscar instancia detectada por nombre o launcher"
            aria-label="Buscar entre instancias detectadas"
          />
        </div>
        <div className="instances-workspace with-right-panel">
          <div className="cards-grid instances-grid-area">
            {filteredInstances.map((item) => (
              <DetectedInstanceCard
                key={item.id}
                item={item}
                selected={selected.includes(item.id)}
                onToggle={() => setSelected((prev) => prev.includes(item.id) ? prev.filter((id) => id !== item.id) : [...prev, item.id])}
              />
            ))}
            {filteredInstances.length === 0 && <article className="instance-card placeholder">Ninguna instancia detectada</article>}
          </div>
          {selected.length > 0 && (
            <ImportSidePanel
              selectedCount={selected.length}
              canImport={selectedItems.some((item) => item.importable)}
              onImport={() => void runImport(buildImportRequests())}
              onClone={() => void executeAction('clonar')}
              onMigrate={() => void executeAction('migrar')}
              onRun={() => void executeAction('ejecutar')}
              onOpenFolder={() => void openSelectedFolder()}
              onClear={() => setSelected([])}
            />
          )}
        </div>
      </section>
      <ImportProgressModal open={running} message={message} progressPercent={executionProgressPercent} />
    </main>
  )
}
