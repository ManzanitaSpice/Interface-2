import { invoke } from '@tauri-apps/api/core'
import { useEffect, useMemo, useState } from 'react'
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
  const { running, message, progressPercent: executionProgressPercent, checkpoints, execute, executeActionBatch } = useImportExecution()
  const [selected, setSelected] = useState<string[]>([])
  const [search, setSearch] = useState('')
  const [loaderFilter, setLoaderFilter] = useState<'all' | 'fabric' | 'forge' | 'neoforge' | 'quilt' | 'vanilla'>('all')
  const [sourceFilter, setSourceFilter] = useState<'all' | 'known' | 'auto'>('all')
  const [modsFilter, setModsFilter] = useState<'all' | 'withMods' | 'withoutMods'>('all')
  const [page, setPage] = useState(1)
  const pageSize = 12

  const selectedItems = useMemo(() => instances.filter((item) => selected.includes(item.id)), [instances, selected])
  const filteredInstances = useMemo(() => {
    const query = search.trim().toLowerCase()
    return instances.filter((item) => {
      const bySearch = !query || item.name.toLowerCase().includes(query) || item.sourceLauncher.toLowerCase().includes(query)
      const normalizedLoader = item.loader.toLowerCase()
      const byLoader = loaderFilter === 'all' || normalizedLoader.includes(loaderFilter)
      const autoSource = item.sourceLauncher.toLowerCase().includes('detectado')
      const bySource = sourceFilter === 'all' || (sourceFilter === 'auto' ? autoSource : !autoSource)
      const modCount = item.modsCount ?? 0
      const byMods = modsFilter === 'all' || (modsFilter === 'withMods' ? modCount > 0 : modCount === 0)
      return bySearch && byLoader && bySource && byMods
    })
  }, [instances, loaderFilter, modsFilter, search, sourceFilter])

  const totalPages = Math.max(1, Math.ceil(filteredInstances.length / pageSize))
  useEffect(() => {
    if (page > totalPages) setPage(totalPages)
  }, [page, totalPages])

  const pagedInstances = useMemo(() => {
    const start = (page - 1) * pageSize
    return filteredInstances.slice(start, start + pageSize)
  }, [filteredInstances, page])

  const toggleSelection = (id: string, withModifier: boolean) => {
    setSelected((prev) => {
      if (withModifier) return prev.includes(id) ? prev.filter((value) => value !== id) : [...prev, id]
      return [id]
    })
  }

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

  const removeSelectedInstances = async () => {
    if (selectedItems.length === 0) return
    const confirmed = window.confirm(`¿Estas seguro de eliminar ${selectedItems.length} instancia(s) detectada(s)? Esta acción elimina completamente la carpeta origen.`)
    if (!confirmed) return
    await executeActionBatch('eliminar_instancia', buildActionRequests('eliminar_instancia'))
    setSelected([])
    await onInstancesChanged?.()
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
          <select value={loaderFilter} onChange={(event) => setLoaderFilter(event.target.value as typeof loaderFilter)}>
            <option value="all">Loader: Todos</option>
            <option value="fabric">Fabric</option>
            <option value="forge">Forge</option>
            <option value="neoforge">NeoForge</option>
            <option value="quilt">Quilt</option>
            <option value="vanilla">Vanilla</option>
          </select>
          <select value={sourceFilter} onChange={(event) => setSourceFilter(event.target.value as typeof sourceFilter)}>
            <option value="all">Origen: Todos</option>
            <option value="known">Origen identificado</option>
            <option value="auto">Auto/descubierto</option>
          </select>
          <select value={modsFilter} onChange={(event) => setModsFilter(event.target.value as typeof modsFilter)}>
            <option value="all">Mods: Todos</option>
            <option value="withMods">Con mods</option>
            <option value="withoutMods">Sin mods</option>
          </select>
        </div>
        <div className="instances-workspace">
          <div className="import-detected-panel">
            <div className="cards-grid instances-grid-area import-cards-grid">
              {pagedInstances.map((item) => (
                <DetectedInstanceCard
                  key={item.id}
                  item={item}
                  selected={selected.includes(item.id)}
                  uiLanguage={uiLanguage}
                  onToggle={(event) => toggleSelection(item.id, event.ctrlKey || event.metaKey)}
                />
              ))}
              {filteredInstances.length === 0 && <article className="instance-card placeholder">{t.empty}</article>}
            </div>
            <footer className="import-pagination">
              <button className="square" onClick={() => setPage((prev) => Math.max(1, prev - 1))} disabled={page <= 1}>Anterior</button>
              <span>Página {page} de {totalPages}</span>
              <button className="square" onClick={() => setPage((prev) => Math.min(totalPages, prev + 1))} disabled={page >= totalPages}>Siguiente</button>
            </footer>
          </div>
        </div>
        {selected.length > 0 && (
          <div className="import-selection-floating">
            <ImportSidePanel
              selectedCount={selected.length}
              canImport={selectedItems.some((item) => item.importable)}
              showBulkActions={selected.length > 1}
              onImport={() => void runImport(buildImportRequests())}
              onClone={() => void executeAction('clonar')}
              onMigrate={() => void executeAction('migrar')}
              onCreateShortcut={() => void executeAction('crear_atajo')}
              onOpenFolder={() => void openSelectedFolder()}
              onDelete={() => void removeSelectedInstances()}
              onClear={() => setSelected([])}
            />
          </div>
        )}
      </section>
      <ImportProgressModal open={running} message={message} progressPercent={executionProgressPercent} checkpoints={checkpoints} />
    </main>
  )
}
