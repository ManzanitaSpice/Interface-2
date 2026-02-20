import { invoke } from '@tauri-apps/api/core'

type Props = {
  label: string
  path: string
}

export function FolderRow({ label, path }: Props) {
  return (
    <div className="folder-route-row">
      <div>
        <strong>{label}</strong>
        <code title={path}>{path}</code>
      </div>
      <div className="folder-route-actions">
        <button onClick={() => void invoke('open_instance_folder', { path })}>📂 Abrir</button>
      </div>
    </div>
  )
}
