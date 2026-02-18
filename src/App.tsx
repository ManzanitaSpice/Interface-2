import { invoke } from '@tauri-apps/api/core'
import { useEffect, useMemo, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from 'react'
import './App.css'

type TopNavItem = 'Mis Modpacks' | 'Novedades' | 'Explorador' | 'Servers' | 'Configuración Global'
type MainPage =
  | 'Inicio'
  | 'Mis Modpacks'
  | 'Novedades'
  | 'Explorador'
  | 'Servers'
  | 'Configuración Global'
  | 'Creador de Instancias'
  | 'Editar Instancia'

type InstanceCard = {
  id: string
  name: string
  group: string
  instanceRoot?: string
}

type CreatorSection =
  | 'Personalizado'
  | 'Vanilla'
  | 'Forge'
  | 'Fabric'
  | 'Quilt'
  | 'NeoForge'
  | 'Snapshot'
  | 'Importar'

type EditSection =
  | 'Ejecución'
  | 'Información'
  | 'Versiones'
  | 'Mods'
  | 'Recursos'
  | 'Java'
  | 'Backups'
  | 'Logs'
  | 'Red'
  | 'Permisos'
  | 'Avanzado'

type CreateInstanceResult = {
  id: string
  name: string
  group: string
  instanceRoot: string
  logs: string[]
}

type ManifestVersion = {
  id: string
  type: string
  url: string
  releaseTime: string
}

type MinecraftVersionDetail = {
  mainClass?: string
  libraries: Array<{ name: string }>
  assets?: string
  assetIndex?: { id?: string; url?: string }
  downloads?: { client?: { url?: string; sha1?: string } }
  arguments?: unknown
  javaVersion?: { majorVersion?: number }
}

type LoaderKey = 'none' | 'neoforge' | 'forge' | 'fabric' | 'quilt'
type MinecraftFilter = 'Releases' | 'Snapshots' | 'Betas' | 'Alfas' | 'Experimentales'

type LoaderVersionItem = {
  version: string
  publishedAt: string
  source: string
  downloadUrl?: string
}

type LoaderChannelFilter = 'Todos' | 'Stable' | 'Latest' | 'Maven'

const topNavItems: TopNavItem[] = ['Mis Modpacks', 'Novedades', 'Explorador', 'Servers', 'Configuración Global']

const creatorSections: CreatorSection[] = ['Personalizado', 'Vanilla', 'Forge', 'Fabric', 'Quilt', 'NeoForge', 'Snapshot', 'Importar']

const editSections: EditSection[] = ['Ejecución', 'Información', 'Versiones', 'Mods', 'Recursos', 'Java', 'Backups', 'Logs', 'Red', 'Permisos', 'Avanzado']

const instanceActions = ['Iniciar', 'Forzar Cierre', 'Editar', 'Cambiar Grupo', 'Carpeta', 'Exportar', 'Copiar', 'Crear atajo']
const defaultGroup = 'Sin grupo'
const sidebarMinWidth = 144
const sidebarMaxWidth = 320
const mojangManifestUrl = 'https://piston-meta.mojang.com/mc/game/version_manifest_v2.json'

function formatIsoDate(iso: string): string {
  if (!iso) return '-'
  return new Date(iso).toLocaleDateString('es-ES')
}

function toJavaMajorOrUndefined(value: number | undefined): number | undefined {
  if (!value || !Number.isFinite(value)) return undefined
  return Math.trunc(value)
}

function mapLoaderToPayload(loader: LoaderKey): string {
  if (loader === 'none') return 'vanilla'
  if (loader === 'quilt') return 'quilt'
  return loader
}

function mapTypeToSpanish(type: string): string {
  if (type === 'release') return 'Release'
  if (type === 'snapshot') return 'Snapshot'
  if (type === 'old_beta') return 'Beta'
  if (type === 'old_alpha') return 'Alfa'
  return type
}

function inferNeoForgeFamily(mcVersion: string): string | null {
  const parts = mcVersion.split('.')
  if (parts.length < 2 || parts[0] !== '1') return null
  const minor = parts[1]
  const patch = parts[2] ?? '0'
  return `${minor}.${patch}`
}

function App() {
  const [activePage, setActivePage] = useState<MainPage>('Mis Modpacks')
  const [cards, setCards] = useState<InstanceCard[]>([])
  const [selectedCreatorSection, setSelectedCreatorSection] = useState<CreatorSection>('Personalizado')
  const [instanceName, setInstanceName] = useState('')
  const [groupName, setGroupName] = useState(defaultGroup)
  const [instanceSearch, setInstanceSearch] = useState('')
  const [minecraftSearch, setMinecraftSearch] = useState('')
  const [loaderSearch, setLoaderSearch] = useState('')
  const [selectedCard, setSelectedCard] = useState<InstanceCard | null>(null)
  const [selectedEditSection, setSelectedEditSection] = useState<EditSection>('Ejecución')
  const [logSearch, setLogSearch] = useState('')
  const [creatorSidebarWidth, setCreatorSidebarWidth] = useState(168)
  const [editSidebarWidth, setEditSidebarWidth] = useState(168)
  const [creationConsoleLogs, setCreationConsoleLogs] = useState<string[]>([])
  const [isCreating, setIsCreating] = useState(false)
  const [manifestVersions, setManifestVersions] = useState<ManifestVersion[]>([])
  const [manifestLoading, setManifestLoading] = useState(false)
  const [manifestError, setManifestError] = useState('')
  const [selectedMcFilter, setSelectedMcFilter] = useState<MinecraftFilter>('Releases')
  const [selectedLoader, setSelectedLoader] = useState<LoaderKey>('none')
  const [selectedMinecraftVersion, setSelectedMinecraftVersion] = useState<ManifestVersion | null>(null)
  const [selectedMinecraftDetail, setSelectedMinecraftDetail] = useState<MinecraftVersionDetail | null>(null)
  const [selectedLoaderVersion, setSelectedLoaderVersion] = useState<LoaderVersionItem | null>(null)
  const [loaderVersions, setLoaderVersions] = useState<LoaderVersionItem[]>([])
  const [loaderLoading, setLoaderLoading] = useState(false)
  const [loaderError, setLoaderError] = useState('')
  const [selectedLoaderFilter, setSelectedLoaderFilter] = useState<LoaderChannelFilter>('Todos')

  useEffect(() => {
    let cancelled = false
    setManifestLoading(true)
    setManifestError('')

    fetch(mojangManifestUrl)
      .then(async (response) => {
        if (!response.ok) {
          throw new Error(`HTTP ${response.status}`)
        }
        return (await response.json()) as { versions?: ManifestVersion[] }
      })
      .then((payload) => {
        if (cancelled) return
        setManifestVersions(payload.versions ?? [])
      })
      .catch((error) => {
        if (cancelled) return
        setManifestError(`No se pudo cargar el manifest oficial de Mojang: ${String(error)}`)
      })
      .finally(() => {
        if (!cancelled) {
          setManifestLoading(false)
        }
      })

    return () => {
      cancelled = true
    }
  }, [])

  useEffect(() => {
    if (!selectedMinecraftVersion) {
      setSelectedMinecraftDetail(null)
      return
    }

    let cancelled = false
    setCreationConsoleLogs((prev) => [...prev, `Descargando version.json oficial de ${selectedMinecraftVersion.id}...`])

    fetch(selectedMinecraftVersion.url)
      .then(async (response) => {
        if (!response.ok) {
          throw new Error(`HTTP ${response.status}`)
        }
        return (await response.json()) as MinecraftVersionDetail
      })
      .then((detail) => {
        if (cancelled) return
        setSelectedMinecraftDetail(detail)
        const libCount = detail.libraries?.length ?? 0
        const javaMajor = detail.javaVersion?.majorVersion ?? 'desconocida'
        const clientUrl = detail.downloads?.client?.url ?? 'sin URL de client.jar'
        setCreationConsoleLogs((prev) => [
          ...prev,
          `version.json cargado: mainClass=${detail.mainClass ?? '-'} | java=${javaMajor} | libraries=${libCount}`,
          `client.jar URL oficial: ${clientUrl}`,
        ])
      })
      .catch((error) => {
        if (cancelled) return
        setSelectedMinecraftDetail(null)
        setCreationConsoleLogs((prev) => [...prev, `Error al descargar version.json: ${String(error)}`])
      })

    return () => {
      cancelled = true
    }
  }, [selectedMinecraftVersion])

  useEffect(() => {
    setSelectedLoaderVersion(null)

    if (!selectedMinecraftVersion || selectedLoader === 'none') {
      setLoaderVersions([])
      setLoaderError('')
      return
    }

    let cancelled = false
    setLoaderLoading(true)
    setLoaderError('')

    const load = async () => {
      if (selectedLoader === 'fabric') {
        const endpoint = `https://meta.fabricmc.net/v2/versions/loader/${encodeURIComponent(selectedMinecraftVersion.id)}`
        const response = await fetch(endpoint)
        if (!response.ok) {
          throw new Error(`Fabric API HTTP ${response.status}`)
        }
        const payload = (await response.json()) as Array<{ loader?: { version?: string }; stable?: boolean }>
        const items: LoaderVersionItem[] = payload
          .map((entry) => ({
            version: entry.loader?.version ?? '',
            publishedAt: '-',
            source: entry.stable ? 'stable' : 'latest',
          }))
          .filter((entry) => Boolean(entry.version))

        if (!cancelled) {
          setLoaderVersions(items)
        }
        return
      }

      if (selectedLoader === 'quilt') {
        const endpoint = `https://meta.quiltmc.org/v3/versions/loader/${encodeURIComponent(selectedMinecraftVersion.id)}`
        const response = await fetch(endpoint)
        if (!response.ok) {
          throw new Error(`Quilt API HTTP ${response.status}`)
        }
        const payload = (await response.json()) as Array<{ loader?: { version?: string } }>
        const items: LoaderVersionItem[] = payload
          .map((entry) => ({
            version: entry.loader?.version ?? '',
            publishedAt: '-',
            source: 'latest',
          }))
          .filter((entry) => Boolean(entry.version))

        if (!cancelled) {
          setLoaderVersions(items)
        }
        return
      }

      if (selectedLoader === 'forge') {
        const metadataUrl = 'https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml'
        const response = await fetch(metadataUrl)
        if (!response.ok) {
          throw new Error(`Forge maven metadata HTTP ${response.status}`)
        }
        const xmlText = await response.text()
        const doc = new DOMParser().parseFromString(xmlText, 'application/xml')
        const versions = Array.from(doc.querySelectorAll('version')).map((node) => node.textContent?.trim() ?? '')
        const prefix = `${selectedMinecraftVersion.id}-`
        const items: LoaderVersionItem[] = versions
          .filter((version) => version.startsWith(prefix))
          .map((version) => {
            const forgeVersion = version.slice(prefix.length)
            return {
              version: forgeVersion,
              publishedAt: '-',
              source: 'maven',
              downloadUrl: `https://maven.minecraftforge.net/net/minecraftforge/forge/${selectedMinecraftVersion.id}-${forgeVersion}/forge-${selectedMinecraftVersion.id}-${forgeVersion}-installer.jar`,
            }
          })

        if (!cancelled) {
          setLoaderVersions(items)
        }
        return
      }

      if (selectedLoader === 'neoforge') {
        const metadataUrl = 'https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml'
        const response = await fetch(metadataUrl)
        if (!response.ok) {
          throw new Error(`NeoForge maven metadata HTTP ${response.status}`)
        }
        const xmlText = await response.text()
        const doc = new DOMParser().parseFromString(xmlText, 'application/xml')
        const versions = Array.from(doc.querySelectorAll('version')).map((node) => node.textContent?.trim() ?? '')
        const family = inferNeoForgeFamily(selectedMinecraftVersion.id)
        const items: LoaderVersionItem[] = versions
          .filter((version) => {
            if (!family) return true
            return version === family || version.startsWith(`${family}.`)
          })
          .map((version) => ({
            version,
            publishedAt: '-',
            source: 'maven',
            downloadUrl: `https://maven.neoforged.net/releases/net/neoforged/neoforge/${version}/neoforge-${version}-installer.jar`,
          }))

        if (!cancelled) {
          setLoaderVersions(items)
        }
      }
    }

    load()
      .catch((error) => {
        if (cancelled) return
        setLoaderVersions([])
        setLoaderError(`No se pudieron resolver versiones de loader para ${selectedLoader}: ${String(error)}`)
      })
      .finally(() => {
        if (!cancelled) {
          setLoaderLoading(false)
        }
      })

    return () => {
      cancelled = true
    }
  }, [selectedLoader, selectedMinecraftVersion])

  const filteredCards = useMemo(() => {
    const term = instanceSearch.trim().toLowerCase()
    if (!term) {
      return cards
    }

    return cards.filter((card) => card.name.toLowerCase().includes(term) || card.group.toLowerCase().includes(term))
  }, [cards, instanceSearch])

  const minecraftRows = useMemo<[string, string, string][]>(() => {
    const searchTerm = minecraftSearch.trim().toLowerCase()
    return manifestVersions
      .filter((version) => {
        if (selectedMcFilter === 'Releases') return version.type === 'release'
        if (selectedMcFilter === 'Snapshots') return version.type === 'snapshot'
        if (selectedMcFilter === 'Betas') return version.type === 'old_beta'
        if (selectedMcFilter === 'Alfas') return version.type === 'old_alpha'
        return version.id.toLowerCase().includes('experimental')
      })
      .filter((version) => !searchTerm || version.id.toLowerCase().includes(searchTerm))
      .map((version) => [version.id, formatIsoDate(version.releaseTime), mapTypeToSpanish(version.type)])
  }, [manifestVersions, minecraftSearch, selectedMcFilter])

  const loaderRows = useMemo<[string, string, string][]>(() => {
    const searchTerm = loaderSearch.trim().toLowerCase()
    return loaderVersions
      .filter((entry) => {
        if (selectedLoaderFilter === 'Todos') return true
        if (selectedLoaderFilter === 'Stable') return entry.source === 'stable'
        if (selectedLoaderFilter === 'Latest') return entry.source === 'latest'
        return entry.source === 'maven'
      })
      .filter((entry) => !searchTerm || entry.version.toLowerCase().includes(searchTerm))
      .map((entry) => [entry.version, entry.publishedAt, entry.source])
  }, [loaderSearch, loaderVersions, selectedLoaderFilter])

  const createInstance = async () => {
    const cleanName = instanceName.trim()
    if (!cleanName || isCreating || !selectedMinecraftVersion) {
      return
    }

    const cleanGroup = groupName.trim() || defaultGroup
    setIsCreating(true)
    setCreationConsoleLogs(['Iniciando creación de instancia...'])

    try {
      const result = await invoke<CreateInstanceResult>('create_instance', {
        payload: {
          name: cleanName,
          group: cleanGroup,
          minecraftVersion: selectedMinecraftVersion.id,
          loader: mapLoaderToPayload(selectedLoader),
          loaderVersion: selectedLoaderVersion?.version ?? '',
          requiredJavaMajor: toJavaMajorOrUndefined(selectedMinecraftDetail?.javaVersion?.majorVersion),
          ramMb: 4096,
          javaArgs: ['-XX:+UseG1GC'],
        },
      })

      const created = { id: result.id, name: result.name, group: result.group, instanceRoot: result.instanceRoot }
      setCards((prev) => [...prev, created])
      setSelectedCard(created)
      setCreationConsoleLogs(result.logs)
      setInstanceName('')
      setGroupName(defaultGroup)
      setActivePage('Mis Modpacks')
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      setCreationConsoleLogs((prev) => [...prev, `Error: ${message}`])
    } finally {
      setIsCreating(false)
    }
  }

  const onTopNavClick = (item: TopNavItem) => {
    setSelectedCard(null)
    if (item === 'Mis Modpacks') {
      setActivePage('Mis Modpacks')
      return
    }
    setActivePage(item)
  }

  const openEditor = () => {
    if (!selectedCard) {
      return
    }

    setSelectedEditSection('Ejecución')
    setActivePage('Editar Instancia')
  }

  const handleInstanceAction = async (action: string) => {
    if (!selectedCard) return

    if (action === 'Editar') {
      openEditor()
      return
    }

    if (action !== 'Carpeta') return

    if (!selectedCard.instanceRoot) {
      setCreationConsoleLogs((prev) => [...prev, `No hay ruta registrada para la instancia ${selectedCard.name}.`])
      return
    }

    try {
      await invoke('open_instance_folder', { path: selectedCard.instanceRoot })
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      setCreationConsoleLogs((prev) => [...prev, `No se pudo abrir la carpeta de la instancia: ${message}`])
    }
  }

  const startSidebarDrag = (
    event: ReactPointerEvent<HTMLDivElement>,
    setter: (value: number) => void,
    initialWidth: number,
    direction: 'right' | 'left',
  ) => {
    event.preventDefault()
    const pointerId = event.pointerId
    const startX = event.clientX

    const onPointerMove = (moveEvent: PointerEvent) => {
      const delta = moveEvent.clientX - startX
      const nextWidth = direction === 'right' ? initialWidth + delta : initialWidth - delta
      const clamped = Math.max(sidebarMinWidth, Math.min(sidebarMaxWidth, nextWidth))
      setter(clamped)
    }

    const stopDrag = () => {
      window.removeEventListener('pointermove', onPointerMove)
      window.removeEventListener('pointerup', stopDrag)
      window.removeEventListener('pointercancel', stopDrag)
    }

    window.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', stopDrag)
    window.addEventListener('pointercancel', stopDrag)

    try {
      event.currentTarget.setPointerCapture(pointerId)
    } catch {
      // No-op if pointer capture is not available.
    }
  }

  return (
    <div className="app-shell">
      {activePage !== 'Creador de Instancias' && activePage !== 'Editar Instancia' && <PrincipalTopBar />}
      {activePage !== 'Creador de Instancias' && activePage !== 'Editar Instancia' && (
        <SecondaryTopBar activePage={activePage} onNavigate={onTopNavClick} />
      )}

      {(activePage === 'Creador de Instancias' || activePage === 'Editar Instancia') && <PrincipalTopBar />}

      {activePage === 'Inicio' && (
        <main className="content content-padded">
          <section className="instances-panel">
            <h1>Panel de Tarjetas de Instancias</h1>
            <p>Espacio preparado para futuras instancias.</p>
            <div className="cards-grid">
              {cards.length === 0 && <article className="instance-card placeholder">Sin instancias creadas aún.</article>}
              {cards.map((card) => (
                <article
                  key={card.id}
                  className={`instance-card clickable ${selectedCard?.id === card.id ? 'active' : ''}`}
                  onClick={() => setSelectedCard(card)}
                >
                  <strong>{card.name}</strong>
                  <span>{card.group}</span>
                </article>
              ))}
            </div>
          </section>
        </main>
      )}

      {activePage === 'Mis Modpacks' && (
        <main className="content content-padded">
          <h1 className="page-title">Mis Modpacks</h1>
          <section className="instances-panel huge-panel">
            <header className="panel-actions">
              <button className="primary" onClick={() => setActivePage('Creador de Instancias')}>
                Crear instancia
              </button>
              <input
                type="search"
                value={instanceSearch}
                onChange={(event) => setInstanceSearch(event.target.value)}
                placeholder="Buscar instancia"
                aria-label="Buscar instancia"
              />
              <button>Más</button>
              <button>Vista</button>
            </header>

            <h2>Panel de Instancias</h2>
            <div className={`instances-workspace ${selectedCard ? 'with-right-panel' : ''}`}>
              <div className="cards-grid instances-grid-area">
                {filteredCards.length === 0 && <article className="instance-card placeholder">No hay instancias para mostrar.</article>}
                {filteredCards.map((card) => (
                  <article
                    key={card.id}
                    className={`instance-card clickable ${selectedCard?.id === card.id ? 'active' : ''}`}
                    onClick={() => setSelectedCard(card)}
                  >
                    <strong>{card.name}</strong>
                    <span>Grupo: {card.group}</span>
                  </article>
                ))}
              </div>

              {selectedCard && (
                <aside className="instance-right-panel">
                  <header>
                    <h3>{selectedCard.name}</h3>
                    <small>Grupo: {selectedCard.group}</small>
                  </header>
                  <div className="instance-right-actions">
                    {instanceActions.map((action) => (
                      <button key={action} className={action === 'Editar' ? 'primary' : ''} onClick={() => handleInstanceAction(action)}>
                        {action}
                      </button>
                    ))}
                  </div>
                </aside>
              )}
            </div>
          </section>
        </main>
      )}

      {activePage !== 'Inicio' &&
        activePage !== 'Mis Modpacks' &&
        activePage !== 'Creador de Instancias' &&
        activePage !== 'Editar Instancia' && (
          <main className="content content-padded">
            <section className="instances-panel">
              <h1>{activePage}</h1>
              <p>Sección en preparación.</p>
            </section>
          </main>
        )}

      {activePage === 'Creador de Instancias' && (
        <main className="creator-layout" style={{ '--sidebar-width': `${creatorSidebarWidth}px` } as CSSProperties}>
          <aside className="compact-sidebar left">
            {creatorSections.map((section) => (
              <button key={section} className={selectedCreatorSection === section ? 'active' : ''} onClick={() => setSelectedCreatorSection(section)}>
                {section}
              </button>
            ))}
          </aside>
          <div
            className="sidebar-resize-handle"
            role="separator"
            aria-label="Redimensionar barra lateral del creador"
            onPointerDown={(event) => startSidebarDrag(event, setCreatorSidebarWidth, creatorSidebarWidth, 'right')}
          />

          <section className="creator-main">
            <header className="third-top-bar">
              <button className="icon-button" aria-label="Icono principal">
                ⛏
              </button>
              <div className="name-fields-with-console">
                <div className="name-fields">
                  <input
                    type="text"
                    placeholder="Nombre de la instancia"
                    value={instanceName}
                    onChange={(event) => setInstanceName(event.target.value)}
                  />
                  <input
                    type="text"
                    placeholder="Grupo (editable, por ejemplo: Vanilla PvP)"
                    value={groupName}
                    onChange={(event) => setGroupName(event.target.value)}
                  />
                </div>
                <aside className="creation-mini-console" role="log" aria-label="Consola de creación">
                  {creationConsoleLogs.length === 0 && <p>Consola lista. Aquí verás la creación e instalación de la instancia.</p>}
                  {creationConsoleLogs.map((line, index) => (
                    <p key={`creation-log-${index}`}>{line}</p>
                  ))}
                </aside>
              </div>
            </header>

            {selectedCreatorSection === 'Personalizado' ? (
              <div className="customized-content">
                <ListInterface
                  title="Interfaz Minecraft"
                  search={minecraftSearch}
                  onSearch={setMinecraftSearch}
                  rows={minecraftRows}
                  selectedKey={selectedMinecraftVersion?.id ?? null}
                  onSelectRow={(rowVersion) => {
                    const found = manifestVersions.find((item) => item.id === rowVersion)
                    if (found) {
                      setSelectedMinecraftVersion(found)
                    }
                  }}
                  rightActions={['Releases', 'Snapshots', 'Betas', 'Alfas', 'Experimentales']}
                  selectedAction={selectedMcFilter}
                  onActionSelect={(value) => setSelectedMcFilter(value as MinecraftFilter)}
                  metaLine={
                    manifestLoading
                      ? 'Cargando version_manifest_v2 oficial de Mojang...'
                      : manifestError
                        ? manifestError
                        : `Fuente oficial: ${mojangManifestUrl}`
                  }
                />
                <ListInterface
                  title="Interfaz Loaders"
                  search={loaderSearch}
                  onSearch={setLoaderSearch}
                  rows={loaderRows}
                  selectedKey={selectedLoaderVersion?.version ?? null}
                  onSelectRow={(rowVersion) => {
                    const found = loaderVersions.find((item) => item.version === rowVersion)
                    if (found) {
                      setSelectedLoaderVersion(found)
                    }
                  }}
                  rightActions={['Todos', 'Stable', 'Latest', 'Maven']}
                  selectedAction={selectedLoaderFilter}
                  onActionSelect={(value) => setSelectedLoaderFilter(value as LoaderChannelFilter)}
                  loaderActions={['Ninguno', 'Neoforge', 'Forge', 'Fabric', 'Quilt']}
                  selectedLoaderAction={{ none: 'Ninguno', neoforge: 'Neoforge', forge: 'Forge', fabric: 'Fabric', quilt: 'Quilt' }[selectedLoader]}
                  onLoaderActionSelect={(value) => {
                    const normalized = value.toLowerCase()
                    if (normalized === 'ninguno') setSelectedLoader('none')
                    else if (normalized === 'neoforge') setSelectedLoader('neoforge')
                    else if (normalized === 'forge') setSelectedLoader('forge')
                    else if (normalized === 'fabric') setSelectedLoader('fabric')
                    else setSelectedLoader('quilt')
                  }}
                  metaLine={
                    !selectedMinecraftVersion
                      ? 'Selecciona primero una versión de Minecraft para resolver loaders compatibles.'
                      : loaderLoading
                        ? `Cargando loaders compatibles para MC ${selectedMinecraftVersion.id}...`
                        : loaderError || `Loader seleccionado: ${selectedLoader}`
                  }
                />
              </div>
            ) : (
              <section className="section-placeholder">
                <h2>{selectedCreatorSection}</h2>
                <p>Configuración específica para esta sección del creador.</p>
              </section>
            )}

            <footer className="creator-footer-actions">
              <button className="primary" onClick={createInstance} disabled={isCreating || !selectedMinecraftVersion}>
                {isCreating ? 'Creando...' : 'Ok'}
              </button>
              <button onClick={() => setActivePage('Mis Modpacks')}>Cancelar</button>
            </footer>
          </section>
        </main>
      )}

      {activePage === 'Editar Instancia' && selectedCard && (
        <main className="edit-instance-layout" style={{ '--sidebar-width': `${editSidebarWidth}px` } as CSSProperties}>
          <aside className="edit-left-sidebar">
            {editSections.map((section) => (
              <button key={section} className={selectedEditSection === section ? 'active' : ''} onClick={() => setSelectedEditSection(section)}>
                {section}
              </button>
            ))}
          </aside>
          <div
            className="sidebar-resize-handle"
            role="separator"
            aria-label="Redimensionar barra lateral de edición"
            onPointerDown={(event) => startSidebarDrag(event, setEditSidebarWidth, editSidebarWidth, 'right')}
          />

          <section className="edit-main-content">
            <header className="edit-top-bar">
              <strong>Editar Instancia: {selectedCard.name}</strong>
              <button onClick={() => setActivePage('Mis Modpacks')}>Volver a Mis Modpacks</button>
            </header>

            {selectedEditSection === 'Ejecución' ? (
              <section className="execution-view">
                <header className="fourth-top-bar">
                  <strong>Ejecución</strong>
                  <span>Panel de control de procesos</span>
                </header>

                <div className="execution-log-console" role="log" aria-label="Consola de logs">
                  {[...Array(18)].map((_, index) => (
                    <p key={`log-${index}`}>
                      [{`12:${(index + 10).toString().padStart(2, '0')}:08`}] Instancia {selectedCard.name} - línea de log #{index + 1}
                    </p>
                  ))}
                </div>

                <input
                  type="search"
                  value={logSearch}
                  onChange={(event) => setLogSearch(event.target.value)}
                  placeholder="Buscar en consola"
                  aria-label="Buscar en consola"
                />

                <footer className="execution-actions">
                  <button className="primary">Iniciar</button>
                  <button>Forzar Cierre</button>
                  <button onClick={() => setActivePage('Mis Modpacks')}>Cerrar</button>
                </footer>
              </section>
            ) : (
              <section className="section-placeholder">
                <h2>{selectedEditSection}</h2>
                <p>Contenido acumulado e información de esta instancia.</p>
              </section>
            )}
          </section>
        </main>
      )}
    </div>
  )
}

type SecondaryTopBarProps = {
  activePage: MainPage
  onNavigate: (item: TopNavItem) => void
}

function PrincipalTopBar() {
  return (
    <header className="top-bar principal">
      <strong>Launcher Control Center</strong>
      <span>Barra principal superior</span>
    </header>
  )
}

function SecondaryTopBar({ activePage, onNavigate }: SecondaryTopBarProps) {
  return (
    <nav className="top-bar secondary">
      {topNavItems.map((item) => (
        <button key={item} onClick={() => onNavigate(item)} className={activePage === item ? 'active' : ''}>
          {item}
        </button>
      ))}
    </nav>
  )
}

type ListInterfaceProps = {
  title: string
  search: string
  onSearch: (value: string) => void
  rows: [string, string, string][]
  rightActions: string[]
  selectedAction: string
  onActionSelect: (action: string) => void
  loaderActions?: string[]
  selectedLoaderAction?: string
  onLoaderActionSelect?: (action: string) => void
  selectedKey: string | null
  onSelectRow: (key: string) => void
  metaLine?: string
}

function ListInterface({
  title,
  search,
  onSearch,
  rows,
  rightActions,
  selectedAction,
  onActionSelect,
  loaderActions,
  selectedLoaderAction,
  onLoaderActionSelect,
  selectedKey,
  onSelectRow,
  metaLine,
}: ListInterfaceProps) {
  return (
    <section className="list-interface">
      <header>
        <h3>{title}</h3>
        <input
          type="search"
          value={search}
          onChange={(event) => onSearch(event.target.value)}
          placeholder={`Buscar en ${title}`}
          aria-label={`Buscar en ${title}`}
        />
      </header>

      <div className="list-interface-layout">
        <div className="table-like">
          <div className="table-head">
            <span>Versión</span>
            <span>Fecha</span>
            <span>Tipo</span>
          </div>
          <div className="table-body-scroll">
            {rows.map((row) => (
              <button className={`table-row table-row-button ${selectedKey === row[0] ? 'active' : ''}`} key={`${title}-${row[0]}`} onClick={() => onSelectRow(row[0])}>
                <span>{row[0]}</span>
                <span>{row[1]}</span>
                <span>{row[2]}</span>
              </button>
            ))}
          </div>
        </div>

        <aside className="mini-right-sidebar buttons-only">
          {loaderActions?.map((action) => (
            <button
              key={`${title}-loader-${action}`}
              className={selectedLoaderAction === action ? 'active' : ''}
              onClick={() => onLoaderActionSelect?.(action)}
            >
              {action}
            </button>
          ))}
          {loaderActions && <hr className="sidebar-divider" />}
          {rightActions.map((action) => (
            <button key={`${title}-${action}`} className={selectedAction === action ? 'active' : ''} onClick={() => onActionSelect(action)}>
              {action}
            </button>
          ))}
        </aside>
      </div>

      {metaLine && <p className="list-interface-meta">{metaLine}</p>}
      {rows.length === 0 && <p className="list-interface-empty">Sin versiones cargadas todavía.</p>}
    </section>
  )
}

export default App
