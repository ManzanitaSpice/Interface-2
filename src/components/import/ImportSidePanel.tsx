type Props = {
  selectedCount: number
  onImport: () => void
  onClear: () => void
}

export function ImportSidePanel({ selectedCount, onImport, onClear }: Props) {
  if (selectedCount === 0) return null
  return (
    <aside className="instance-right-panel">
      <h3>IMPORTAR SELECCIÓN</h3>
      <p>{selectedCount} instancias seleccionadas</p>
      <button className="primary" onClick={onImport}>✅ Importar ahora</button>
      <button onClick={onClear}>❌ Deseleccionar todo</button>
    </aside>
  )
}
