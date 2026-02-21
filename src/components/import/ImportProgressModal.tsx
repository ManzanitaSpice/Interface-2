import type { ImportFocusStatus } from '../../types/import'

type Props = { open: boolean; message: string; progressPercent?: number; checkpoints?: ImportFocusStatus[] }

export function ImportProgressModal({ open, message, progressPercent = 0, checkpoints = [] }: Props) {
  if (!open) return null
  const percent = Math.max(0, Math.min(100, progressPercent))
  return (
    <div className="floating-modal-overlay" role="dialog" aria-modal="true" aria-label="Importando instancias">
      <div className="floating-modal">
        <h3>Importando instancias...</h3>
        <p>{message || 'No cierres el launcher durante la importación.'}</p>
        <div className="creation-progress-wrap"><div className="creation-progress-fill" style={{ width: `${percent}%` }} /></div>
        <small>{percent}%</small>
        {checkpoints.length > 0 && (
          <div className="import-focus-row" aria-label="estado por focos">
            {checkpoints.map((point) => (
              <div key={point.key} className={`import-focus-dot ${point.status}`} title={point.label} aria-label={point.label} />
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
