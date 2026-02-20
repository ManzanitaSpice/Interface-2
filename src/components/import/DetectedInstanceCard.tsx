import type { DetectedInstance } from '../../types/import'

type Props = {
  item: DetectedInstance
  selected: boolean
  onToggle: () => void
}

export function DetectedInstanceCard({ item, selected, onToggle }: Props) {
  return (
    <article
      className={`instance-card clickable ${selected ? 'active' : ''} ${!item.importable ? 'is-dim' : ''}`}
      onClick={() => item.importable && onToggle()}
      title={item.importWarnings.join(', ')}
    >
      <div
        className="instance-card-icon hero"
        style={item.iconPath ? { backgroundImage: `url(${item.iconPath})` } : undefined}
      >
        {!item.iconPath ? '📦' : null}
      </div>
      <strong className="instance-card-title">{item.name}</strong>
      <div className="instance-card-meta">
        <small>{item.sourceLauncher}</small>
        <small>MC {item.minecraftVersion}</small>
        <small>{item.loader} {item.loaderVersion}</small>
        <small>{item.modsCount ?? 0} mods</small>
      </div>
    </article>
  )
}
