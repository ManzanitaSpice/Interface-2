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
  uiLanguage: 'es' | 'en' | 'pt'
}

const text = {
  es: { search: 'Buscar instancia detectada por nombre o launcher', searchAria: 'Buscar entre instancias detectadas', empty: 'Ninguna instancia detectada' },
  en: { search: 'Search detected instance by name or launcher', searchAria: 'Search among detected instances', empty: 'No detected instances' },
  pt: { search: 'Buscar instância detectada por nome ou launcher', searchAria: 'Buscar entre instâncias detectadas', empty: 'Nenhuma instância detectada' },
} as const

export function ImportPage({ onInstancesChanged, uiLanguage }: Props) {
  const t = text[uiLanguage]
  const { instances, status, progressPercent, isScanning, keepDetected, setKeepDetected, scan, clear } = useImportScanner()
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
      sourceLauncher: item.sourceLauncher,
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
        sourceLauncher: first.sourceLauncher,
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
        <ScanStatusBar status={status} progressPercent={progressPercent} isScanning={isScanning} />
        <div className="instance-search-row">
          <input
            className="instance-search-compact"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder={t.search}
            aria-label={t.searchAria}
          />
        </div>
        <div className="instances-workspace">
          <div className="cards-grid instances-grid-area import-cards-grid">
            {filteredInstances.map((item) => (
              <DetectedInstanceCard
                key={item.id}
                item={item}
                selected={selected.includes(item.id)}
                uiLanguage={uiLanguage}
                onToggle={() => setSelected((prev) => prev.includes(item.id) ? prev.filter((id) => id !== item.id) : [...prev, item.id])}
              />
            ))}
            {filteredInstances.length === 0 && <article className="instance-card placeholder">{t.empty}</article>}
          </div>
        </div>
        {selected.length > 0 && (
          <div className="import-selection-floating">
            <ImportSidePanel
              selectedCount={selected.length}
              canImport={selectedItems.some((item) => item.importable)}
              onImport={() => void runImport(buildImportRequests())}
              onClone={() => void executeAction('clonar')}
              onMigrate={() => void executeAction('migrar')}
              onCreateShortcut={() => void executeAction('crear_atajo')}
              onOpenFolder={() => void openSelectedFolder()}
              onClear={() => setSelected([])}
            />
          </div>
        )}
      </section>
      <ImportProgressModal open={running} message={message} progressPercent={executionProgressPercent} />
    </main>
  )
}
