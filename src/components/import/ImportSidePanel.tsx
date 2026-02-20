type Props = {
  selectedCount: number
  canImport: boolean
  onImport: () => void
  onClear: () => void
  onClone: () => void
  onMigrate: () => void
  onRun: () => void
  onOpenFolder: () => void
}

export function ImportSidePanel({ selectedCount, canImport, onImport, onClear, onClone, onMigrate, onRun, onOpenFolder }: Props) {
  return (
    <aside className="instance-right-panel">
      <h3>IMPORTAR SELECCIÓN</h3>
      <p>{selectedCount} instancias seleccionadas</p>
      <button className="primary" onClick={onImport} disabled={!canImport}>✅ Importar ahora</button>
      <button onClick={onClone} disabled={!canImport}>🧬 Clonar instancia</button>
      <button onClick={onMigrate} disabled={!canImport}>🚚 Migrar instancia</button>
      <button onClick={onRun} disabled={!canImport}>▶️ Ejecutar instancia</button>
      <button onClick={onOpenFolder} disabled={selectedCount === 0}>📁 Abrir carpeta</button>
      <button onClick={onClear} disabled={selectedCount === 0}>❌ Deseleccionar todo</button>
    </aside>
  )
}
