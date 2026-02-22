type Props = {
  selectedCount: number
  canImport: boolean
  showBulkActions: boolean
  onImport: () => void
  onClone: () => void
  onMigrate: () => void
  onCreateShortcut: () => void
  onOpenFolder: () => void
  onDelete: () => void
  onClear: () => void
}

export function ImportSidePanel({ selectedCount, canImport, showBulkActions, onImport, onClone, onMigrate, onCreateShortcut, onOpenFolder, onDelete, onClear }: Props) {
  return (
    <aside className="instance-right-panel import-selection-panel">
      <h3>OPERACIONES DE SELECCIÓN</h3>
      <p>{selectedCount} instancia(s) seleccionada(s)</p>
      <p className="filter-label">Flujo principal</p>
      <button className="primary import-action" onClick={onImport} disabled={!canImport}>Importar ahora</button>
      <button className="import-action" onClick={onCreateShortcut} disabled>Próximamente</button>
      <p className="filter-label">Transformar</p>
      <button className="import-action" onClick={onClone} disabled={!canImport}>Clonar instancia</button>
      <button className="import-action" onClick={onMigrate} disabled={!canImport}>Migrar instancia</button>
      <p className="filter-label">Utilidades</p>
      <button className="import-action" onClick={onOpenFolder} disabled={selectedCount === 0}>Abrir carpeta origen</button>
      {showBulkActions && <button className="import-action danger" onClick={onDelete} disabled={selectedCount === 0}>Eliminar instancias</button>}
      {showBulkActions && <button className="import-action danger" onClick={onClear} disabled={selectedCount === 0}>Limpiar selección</button>}
    </aside>
  )
}
