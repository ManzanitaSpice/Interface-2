import { useMemo, useState } from 'react'
import './App.css'

type TopNavItem = 'Mis Modpacks' | 'Novedades' | 'Explorador' | 'Servers' | 'Configuración Global'
type MainPage = 'Inicio' | 'Mis Modpacks' | 'Novedades' | 'Explorador' | 'Servers' | 'Configuración Global' | 'Creador de Instancias'

type InstanceCard = {
  id: number
  name: string
  group: string
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

const topNavItems: TopNavItem[] = [
  'Mis Modpacks',
  'Novedades',
  'Explorador',
  'Servers',
  'Configuración Global',
]

const creatorSections: CreatorSection[] = [
  'Personalizado',
  'Vanilla',
  'Forge',
  'Fabric',
  'Quilt',
  'NeoForge',
  'Snapshot',
  'Importar',
]

const rightSidebarContent: Record<CreatorSection, string[]> = {
  Personalizado: ['Resumen', 'Compatibilidad', 'Dependencias', 'Perfil', 'Presets', 'Ayuda'],
  Vanilla: ['Versionado', 'Java', 'Optimización', 'Recursos', 'Notas', 'Exportar'],
  Forge: ['Instalador', 'Canales', 'Librerías', 'Mods base', 'Logs', 'Guía'],
  Fabric: ['Loader', 'API', 'Rendimiento', 'Assets', 'Sincronizar', 'Tips'],
  Quilt: ['QSL', 'Parámetros', 'Migración', 'Entorno', 'Paquetes', 'Wiki'],
  NeoForge: ['Core', 'Parcheado', 'Versiones', 'Plantillas', 'Pruebas', 'Estado'],
  Snapshot: ['Snapshots', 'Historial', 'Comparador', 'Cambios', 'Riesgos', 'Backups'],
  Importar: ['Origen', 'Metadatos', 'Validación', 'Conflictos', 'Preview', 'Importar'],
}

const groups = ['Survival', 'PvP', 'Técnico', 'Aventura', 'Sin grupo']

function App() {
  const [activePage, setActivePage] = useState<MainPage>('Inicio')
  const [cards, setCards] = useState<InstanceCard[]>([])
  const [selectedCreatorSection, setSelectedCreatorSection] = useState<CreatorSection>('Personalizado')
  const [instanceName, setInstanceName] = useState('')
  const [groupName, setGroupName] = useState(groups[0])
  const [instanceSearch, setInstanceSearch] = useState('')
  const [minecraftSearch, setMinecraftSearch] = useState('')
  const [loaderSearch, setLoaderSearch] = useState('')

  const filteredCards = useMemo(() => {
    const term = instanceSearch.trim().toLowerCase()
    if (!term) {
      return cards
    }

    return cards.filter(
      (card) => card.name.toLowerCase().includes(term) || card.group.toLowerCase().includes(term),
    )
  }, [cards, instanceSearch])

  const createInstance = () => {
    const cleanName = instanceName.trim()
    if (!cleanName) {
      return
    }

    setCards((prev) => [...prev, { id: Date.now(), name: cleanName, group: groupName }])
    setInstanceName('')
    setGroupName(groups[0])
    setActivePage('Mis Modpacks')
  }

  const onTopNavClick = (item: TopNavItem) => {
    if (item === 'Mis Modpacks') {
      setActivePage('Mis Modpacks')
      return
    }
    setActivePage(item)
  }

  return (
    <div className="app-shell">
      {activePage !== 'Creador de Instancias' && <PrincipalTopBar />}
      {activePage !== 'Creador de Instancias' && (
        <SecondaryTopBar activePage={activePage} onNavigate={onTopNavClick} />
      )}

      {activePage === 'Creador de Instancias' && <PrincipalTopBar />}

      {activePage === 'Inicio' && (
        <main className="content content-padded">
          <section className="instances-panel">
            <h1>Panel de Tarjetas de Instancias</h1>
            <p>Espacio preparado para futuras instancias.</p>
            <div className="cards-grid">
              {cards.length === 0 && <article className="instance-card placeholder">Sin instancias creadas aún.</article>}
              {cards.map((card) => (
                <article key={card.id} className="instance-card">
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
            <div className="cards-grid">
              {filteredCards.length === 0 && (
                <article className="instance-card placeholder">No hay instancias para mostrar.</article>
              )}
              {filteredCards.map((card) => (
                <article key={card.id} className="instance-card">
                  <strong>{card.name}</strong>
                  <span>Grupo: {card.group}</span>
                </article>
              ))}
            </div>
          </section>
        </main>
      )}

      {activePage !== 'Inicio' && activePage !== 'Mis Modpacks' && activePage !== 'Creador de Instancias' && (
        <main className="content content-padded">
          <section className="instances-panel">
            <h1>{activePage}</h1>
            <p>Sección en preparación.</p>
          </section>
        </main>
      )}

      {activePage === 'Creador de Instancias' && (
        <main className="creator-layout">
          <aside className="compact-sidebar left">
            {creatorSections.map((section) => (
              <button
                key={section}
                className={selectedCreatorSection === section ? 'active' : ''}
                onClick={() => setSelectedCreatorSection(section)}
              >
                {section}
              </button>
            ))}
          </aside>

          <section className="creator-main">
            <header className="third-top-bar">
              <button className="icon-button" aria-label="Icono principal">
                ⛏
              </button>
              <div className="name-fields">
                <input
                  type="text"
                  placeholder="Nombre de la instancia"
                  value={instanceName}
                  onChange={(event) => setInstanceName(event.target.value)}
                />
                <select value={groupName} onChange={(event) => setGroupName(event.target.value)}>
                  {groups.map((group) => (
                    <option key={group} value={group}>
                      {group}
                    </option>
                  ))}
                </select>
              </div>
            </header>

            {selectedCreatorSection === 'Personalizado' ? (
              <div className="customized-content">
                <ListInterface
                  title="Interfaz Minecraft"
                  search={minecraftSearch}
                  onSearch={setMinecraftSearch}
                  rows={[
                    ['1.21.4', '2025-01-15', 'Release'],
                    ['1.20.6', '2024-04-29', 'Release'],
                    ['1.20.1', '2023-06-12', 'LTS'],
                  ]}
                />
                <ListInterface
                  title="Interfaz Loaders"
                  search={loaderSearch}
                  onSearch={setLoaderSearch}
                  rows={[
                    ['Forge 51.0', '2025-01-10', 'Estable'],
                    ['Fabric 0.16', '2024-12-14', 'Estable'],
                    ['NeoForge 21.4', '2025-02-02', 'Beta'],
                  ]}
                />
              </div>
            ) : (
              <section className="section-placeholder">
                <h2>{selectedCreatorSection}</h2>
                <p>Configuración específica para esta sección del creador.</p>
              </section>
            )}

            <footer className="creator-footer-actions">
              <button className="primary" onClick={createInstance}>
                Ok
              </button>
              <button onClick={() => setActivePage('Mis Modpacks')}>Cancelar</button>
            </footer>
          </section>

          <aside className="compact-sidebar right">
            {rightSidebarContent[selectedCreatorSection].map((item) => (
              <button key={item}>{item}</button>
            ))}
          </aside>
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
        <button
          key={item}
          onClick={() => onNavigate(item)}
          className={activePage === item ? 'active' : ''}
        >
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
}

function ListInterface({ title, search, onSearch, rows }: ListInterfaceProps) {
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
          {rows.map((row) => (
            <div className="table-row" key={`${title}-${row[0]}`}>
              <span>{row[0]}</span>
              <span>{row[1]}</span>
              <span>{row[2]}</span>
            </div>
          ))}
        </div>

        <aside className="mini-right-sidebar">
          {['Filtro', 'Orden', 'Tag', 'Previa', 'Fix', 'Pin'].map((item) => (
            <button key={`${title}-${item}`}>{item}</button>
          ))}
        </aside>
      </div>
    </section>
  )
}

export default App
