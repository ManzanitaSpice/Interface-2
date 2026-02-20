type Props = {
  status: string
  detectedCount: number
  selectedCount: number
  isScanning: boolean
  keepDetected: boolean
  onToggleKeepDetected: () => void
  onScan: () => void
  onClear: () => void
}

export function ImportToolbar({ status, detectedCount, selectedCount, isScanning, keepDetected, onToggleKeepDetected, onScan, onClear }: Props) {
  return (
    <header className="panel-actions import-toolbar">
      <div className="import-toolbar-summary">
        <strong>Importador</strong>
        <small>{status}</small>
      </div>
      <div className="import-toolbar-badges">
        <span>Detectadas: {detectedCount}</span>
        <span>Seleccionadas: {selectedCount}</span>
      </div>
      <button onClick={onScan} disabled={isScanning}>{isScanning ? '⏳ Escaneando...' : '🔍 Detectar'}</button>
      <button onClick={onClear}>🗑 Limpiar panel</button>
      <button className={keepDetected ? 'primary' : ''} onClick={onToggleKeepDetected}>{keepDetected ? '✅ Mantener' : '⬜ Mantener'}</button>
    </header>
  )
}
