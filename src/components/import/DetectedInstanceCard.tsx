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

const normalizeLoader = (loader: string) => {
  const value = loader.trim().toLowerCase()
  if (!value || value === 'vanilla') return { id: 'vanilla', label: 'Vanilla', icon: '🟩' }
  if (value.includes('fabric')) return { id: 'fabric', label: 'Fabric', icon: '🧵' }
  if (value.includes('neoforge')) return { id: 'neoforge', label: 'NeoForge', icon: '🔶' }
  if (value.includes('forge')) return { id: 'forge', label: 'Forge', icon: '⚒️' }
  if (value.includes('quilt') || value.includes('quilit')) return { id: 'quilt', label: 'Quilt', icon: '🧶' }
  return { id: value, label: loader, icon: '📦' }
}

export function DetectedInstanceCard({ item, selected, onToggle }: Props) {
  const icon = resolveIcon(item.iconPath)
  const loaderInfo = normalizeLoader(item.loader || 'vanilla')
  const loaderLabel = `${loaderInfo.label} ${item.loaderVersion || ''}`.trim()
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
      <div className="instance-card-meta">
        <small>Origen: {item.sourceLauncher}</small>
        <small>MC {item.minecraftVersion}</small>
        <small>Loader: {loaderInfo.icon} {loaderLabel}</small>
        <small>Peso real: {sizeLabel}</small>
        <small>{item.modsCount ?? 0} mods</small>
      </div>
    </article>
  )
}
