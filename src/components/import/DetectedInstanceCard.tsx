import { convertFileSrc } from '@tauri-apps/api/core'
import type { DetectedInstance } from '../../types/import'

type Props = {
  item: DetectedInstance
  selected: boolean
  onToggle: () => void
}

const resolveIcon = (iconPath?: string | null) => {
  if (!iconPath) return ''
  if (iconPath.startsWith('http') || iconPath.startsWith('data:image') || iconPath.startsWith('asset:')) return iconPath
  try {
    return convertFileSrc(iconPath)
  } catch {
    return iconPath
  }
}

const normalizeLoaderName = (loader: string) => {
  const key = loader.trim().toLowerCase()
  if (key.includes('fabric')) return 'Fabric'
  if (key.includes('neoforge')) return 'NeoForge'
  if (key.includes('forge')) return 'Forge'
  if (key.includes('quilt') || key.includes('quilit')) return 'Quilt'
  return 'Vanilla'
}

export function DetectedInstanceCard({ item, selected, onToggle }: Props) {
  const icon = resolveIcon(item.iconPath)
  const loaderName = normalizeLoaderName(item.loader)
  const loaderLabel = `${loaderName}${item.loaderVersion && item.loaderVersion !== '-' ? ` ${item.loaderVersion}` : ''}`
  const sizeLabel = item.sizeMb && item.sizeMb > 0 ? `${item.sizeMb} MB` : 'Tamaño no detectado'

  return (
    <article
      className={`instance-card clickable ${selected ? 'active' : ''} ${!item.importable ? 'is-dim' : ''}`}
      onClick={() => item.importable && onToggle()}
      title={item.importWarnings.join(', ')}
    >
      <div className="instance-card-icon hero" style={icon ? { backgroundImage: `url(${icon})` } : undefined}>
        {!icon ? '📦' : null}
      </div>
      <strong className="instance-card-title">{item.name}</strong>
      <span className="import-loader-chip">{loaderName}</span>
      <div className="instance-card-meta">
        <small>Origen: {item.sourceLauncher}</small>
        <small>MC {item.minecraftVersion}</small>
        <small>Loader: {loaderLabel}</small>
        <small>Peso real: {sizeLabel}</small>
        <small>{item.modsCount ?? 0} mods</small>
      </div>
    </article>
  )
}
