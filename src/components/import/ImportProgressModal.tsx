type Props = { open: boolean; message: string }

export function ImportProgressModal({ open, message }: Props) {
  if (!open) return null
  return (
    <div className="floating-modal-overlay" role="dialog" aria-modal="true" aria-label="Importando instancias">
      <div className="floating-modal">
        <h3>Importando instancias...</h3>
        <p>{message || 'No cierres el launcher durante la importación.'}</p>
      </div>
    </div>
  )
}
