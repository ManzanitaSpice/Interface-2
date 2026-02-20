import { useMemo, useState } from 'react'
import { DetectedInstanceCard } from '../components/import/DetectedInstanceCard'
import { ImportProgressModal } from '../components/import/ImportProgressModal'
import { ImportSidePanel } from '../components/import/ImportSidePanel'
import { ImportToolbar } from '../components/import/ImportToolbar'
import { ScanStatusBar } from '../components/import/ScanStatusBar'
import { useImportExecution } from '../hooks/useImportExecution'
import { useImportScanner } from '../hooks/useImportScanner'

export function ImportPage() {
  const { instances, status, progressPercent, scanLogs, isScanning, scan, clear } = useImportScanner()
  const { running, message, execute } = useImportExecution()
  const [selected, setSelected] = useState<string[]>([])

  const selectedItems = useMemo(() => instances.filter((item) => selected.includes(item.id)), [instances, selected])

  return (
    <main className="content content-padded">
      <section className="instances-panel huge-panel">
        <ImportToolbar status={status} onScan={() => void scan()} onClear={clear} />
        <ScanStatusBar status={status} progressPercent={progressPercent} scanLogs={scanLogs} isScanning={isScanning} />
        <div className="instances-workspace with-right-panel">
          <div className="cards-grid instances-grid-area">
            {instances.map((item) => (
              <DetectedInstanceCard
                key={item.id}
                item={item}
                selected={selected.includes(item.id)}
                onToggle={() => setSelected((prev) => prev.includes(item.id) ? prev.filter((id) => id !== item.id) : [...prev, item.id])}
              />
            ))}
            {instances.length === 0 && <article className="instance-card placeholder">Ninguna instancia detectada</article>}
          </div>
          <ImportSidePanel
            selectedCount={selected.length}
            onImport={() => void execute(selectedItems.map((item) => ({
              detectedInstanceId: item.id,
              targetName: item.name,
              targetGroup: 'Sin grupo',
              ramMb: 4096,
              copyMods: true,
              copyWorlds: true,
              copyResourcepacks: true,
              copyScreenshots: false,
              copyLogs: false,
            })))}
            onClear={() => setSelected([])}
          />
        </div>
      </section>
      <ImportProgressModal open={running} message={message} />
    </main>
  )
}
