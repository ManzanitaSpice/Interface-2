import { convertFileSrc } from '@tauri-apps/api/core'
import type { DetectedInstance } from '../../types/import'

type Props = {
  item: DetectedInstance
  selected: boolean
  onToggle: () => void
  uiLanguage: 'es' | 'en' | 'pt'
}

const text = {
  es: { source: 'Origen', loader: 'Loader', realSize: 'Peso real', unknownSize: 'Tamaño no detectado', mods: 'mods' },
  en: { source: 'Source', loader: 'Loader', realSize: 'Real size', unknownSize: 'Size not detected', mods: 'mods' },
  pt: { source: 'Origem', loader: 'Loader', realSize: 'Tamanho real', unknownSize: 'Tamanho não detectado', mods: 'mods' },
} as const

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

export function DetectedInstanceCard({ item, selected, onToggle, uiLanguage }: Props) {
  const t = text[uiLanguage]
  const icon = resolveIcon(item.iconPath)
  const loaderInfo = normalizeLoader(item.loader || 'vanilla')
  const loaderLabel = `${loaderInfo.label} ${item.loaderVersion || ''}`.trim()
  const sizeLabel = item.sizeMb && item.sizeMb > 0 ? `${item.sizeMb} MB` : t.unknownSize

  const displayName = item.name?.trim() || 'Instancia sin nombre'
  const modsCount = item.modsCount ?? 0

  return (
    <article
      className={`instance-card clickable ${selected ? 'active' : ''} ${!item.importable ? 'is-dim' : ''}`}
      onClick={() => item.importable && onToggle()}
      title={item.importWarnings.join(', ')}
    >
      {selected && <span className="instance-selected-chip">✓ Seleccionada</span>}
      <div className="instance-card-icon hero">
        {icon ? (
          <img src={icon} alt={displayName} loading="lazy" referrerPolicy="no-referrer" />
        ) : (
          '📦'
        )}
      </div>
      <strong className="instance-card-title">{displayName}</strong>
      <div className="instance-card-meta">
        <small>{t.source}: {item.sourceLauncher}</small>
        <small>MC {item.minecraftVersion}</small>
        <small>{t.loader}: {loaderInfo.icon} {loaderLabel}</small>
        <small>{t.realSize}: {sizeLabel}</small>
        <small>{modsCount} {t.mods}</small>
      </div>
    </article>
  )
}
