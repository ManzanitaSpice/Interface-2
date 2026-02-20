type Progress = { step: string; completed: number; total: number; message: string }

type Props = {
  title: string
  description: string
  open: boolean
  pendingPath: string
  progress: Progress | null
  onClose: () => void
  onMigrate: () => void
  onOnlyPath: () => void
}

export function MigrationModal({ title, description, open, pendingPath, progress, onClose, onMigrate, onOnlyPath }: Props) {
  if (!open) return null

  const percent = progress ? Math.round((progress.completed / Math.max(progress.total, 1)) * 100) : 0

  return (
    <div className="floating-modal-overlay" role="dialog" aria-modal="true" aria-label={title}>
      <div className="floating-modal">
        <h3>{title}</h3>
        <p>Nueva ubicación:</p>
        <code>{pendingPath}</code>
        <p>{description}</p>
        {progress && (
          <>
            <p>{progress.message}</p>
            <div className="creation-progress-wrap"><div className="creation-progress-fill" style={{ width: `${percent}%` }} /></div>
            <small>{progress.completed} / {progress.total}</small>
          </>
        )}
        {!progress && (
          <div className="modal-actions">
            <button className="primary" onClick={onMigrate}>Migrar todo</button>
            <button onClick={onOnlyPath}>Solo cambiar ruta</button>
            <button className="danger" onClick={onClose}>Cancelar</button>
          </div>
        )}
      </div>
    </div>
  )
}
