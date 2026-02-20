type Props = {
  selectedCount: number
  canImport: boolean
  onImport: () => void
  onClear: () => void
  onClone: () => void
  onMigrate: () => void
  onCreateShortcut: () => void
  onOpenFolder: () => void
}

export function ImportSidePanel({ selectedCount, canImport, onImport, onClear, onClone, onMigrate, onCreateShortcut, onOpenFolder }: Props) {
  return (
    <aside className="instance-right-panel import-selection-panel">
      <h3>IMPORTAR SELECCIÓN</h3>
      <p>{selectedCount} instancias seleccionadas</p>
      <button className="primary import-action" onClick={onImport} disabled={!canImport}>✅ Importar ahora</button>
      <button className="import-action" onClick={onClone} disabled={!canImport}>🧬 Clonar instancia</button>
      <button className="import-action" onClick={onMigrate} disabled={!canImport}>🚚 Migrar instancia</button>
      <button className="import-action" onClick={onCreateShortcut} disabled={!canImport}>🔗 Crear atajo</button>
      <button className="import-action" onClick={onOpenFolder} disabled={selectedCount === 0}>📁 Abrir carpeta</button>
      <button className="import-action danger" onClick={onClear} disabled={selectedCount === 0}>❌ Deseleccionar todo</button>
    </aside>
  )
}
