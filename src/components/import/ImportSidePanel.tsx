type Props = {
  selectedCount: number
  canImport: boolean
  onImport: () => void
  onClear: () => void
}

export function ImportSidePanel({ selectedCount, canImport, onImport, onClear }: Props) {
  return (
    <aside className="instance-right-panel">
      <h3>IMPORTAR SELECCIÓN</h3>
      <p>{selectedCount} instancias seleccionadas</p>
      <button className="primary" onClick={onImport} disabled={!canImport}>✅ Importar ahora</button>
      <button onClick={onClear} disabled={selectedCount === 0}>❌ Deseleccionar todo</button>
    </aside>
  )
}
