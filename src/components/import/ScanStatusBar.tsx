type Props = {
  status: string
  progressPercent: number
  scanLogs: string[]
  isScanning: boolean
}

export function ScanStatusBar({ status, progressPercent, scanLogs, isScanning }: Props) {
  return (
    <div className="scan-status-wrap">
      <div className="updates-status-bar">{status}</div>
      <div className="scan-progress-track" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={progressPercent}>
        <div className="scan-progress-fill" style={{ width: `${Math.max(0, Math.min(100, progressPercent))}%` }} />
      </div>
      <small className="scan-progress-caption">{isScanning ? `Escaneando: ${progressPercent}%` : `Estado: ${progressPercent}%`}</small>
      <div className="scan-log-box">
        {scanLogs.length === 0 ? <small>Sin actividad todavía.</small> : scanLogs.slice(-8).map((line, index) => <small key={`${line}-${index}`}>{line}</small>)}
      </div>
    </div>
  )
}
