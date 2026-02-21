type Props = {
  selectedCount: number
  canImport: boolean
  onImport: () => void
  onClear: () => void
  onClone: () => void
  onMigrate: () => void
  onCreateShortcut: () => void
  onOpenFolder: () => void
  onDelete: () => void
}

export function ImportSidePanel({ selectedCount, canImport, onImport, onClear, onClone, onMigrate, onCreateShortcut, onOpenFolder, onDelete }: Props) {
  return (
    <aside className="instance-right-panel import-selection-panel">
      <h3>OPERACIONES DE SELECCIÓN</h3>
      <p>{selectedCount} instancias seleccionadas</p>
      <p className="filter-label">Flujo principal</p>
      <button className="primary import-action" onClick={onImport} disabled={!canImport}>✅ Importar ahora</button>
      <button className="import-action" onClick={onCreateShortcut} disabled={!canImport}>🔗 Crear atajo</button>
      <p className="filter-label">Transformar</p>
      <button className="import-action" onClick={onClone} disabled={!canImport}>🧬 Clonar instancia</button>
      <button className="import-action" onClick={onMigrate} disabled={!canImport}>🚚 Migrar instancia</button>
      <p className="filter-label">Utilidades</p>
      <button className="import-action" onClick={onOpenFolder} disabled={selectedCount === 0}>📁 Abrir carpeta origen</button>
      <button className="import-action danger" onClick={onDelete} disabled={selectedCount === 0}>🗑 Eliminar instancia(s)</button>
      <button className="import-action danger" onClick={onClear} disabled={selectedCount === 0}>❌ Deseleccionar todo</button>
    </aside>
  )
}
