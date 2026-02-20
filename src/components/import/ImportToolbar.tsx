type Props = {
  status: string
  onScan: () => void
  onClear: () => void
}

export function ImportToolbar({ status, onScan, onClear }: Props) {
  return (
    <header className="panel-actions">
      <button onClick={onScan}>🔍 Detector Automático</button>
      <button disabled>📂 Importar Específico</button>
      <button onClick={onClear}>🗑 Limpiar panel</button>
      <button title="Soporta carpetas de CurseForge, Modrinth, Prism, MultiMC, zips y mrpack.">ℹ Ayuda</button>
      <span>{status}</span>
    </header>
  )
}
