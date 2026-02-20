type Props = { open: boolean; message: string; progressPercent?: number }

export function ImportProgressModal({ open, message, progressPercent = 0 }: Props) {
  if (!open) return null
  const percent = Math.max(0, Math.min(100, progressPercent))
  return (
    <div className="floating-modal-overlay" role="dialog" aria-modal="true" aria-label="Importando instancias">
      <div className="floating-modal">
        <h3>Importando instancias...</h3>
        <p>{message || 'No cierres el launcher durante la importación.'}</p>
        <div className="creation-progress-wrap"><div className="creation-progress-fill" style={{ width: `${percent}%` }} /></div>
        <small>{percent}%</small>
      </div>
    </div>
  )
}
