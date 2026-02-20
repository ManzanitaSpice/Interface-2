import { invoke } from '@tauri-apps/api/core'
import { useEffect, useMemo, useState } from 'react'

type Category = 'All' | 'Modpacks' | 'Mods' | 'DataPacks' | 'Resource Packs' | 'Shaders' | 'Worlds' | 'Addons' | 'Customizacion'
type SortMode = 'Relevancia' | 'Popularidad' | 'Ultima Actualizacion' | 'Actualizacion Estable' | 'Mas Descargas' | 'Nombre' | 'Autor'
type ViewMode = 'lista' | 'tablero' | 'titulos'
type Platform = 'Todas' | 'Curseforge' | 'Modrinth'
type LoaderFilter = 'Todos' | 'Fabric' | 'Forge' | 'Neoforge' | 'Quilt'

type ExplorerItem = {
  id: string
  source: 'CurseForge' | 'Modrinth'
  title: string
  description: string
  image: string
  author: string
  downloads: number
  updatedAt: string
  size: string
  minecraftVersions: string[]
  loaders: string[]
  projectType: string
  tags: string[]
}

const categoryToProjectType: Record<Category, string | null> = {
  All: null,
  Modpacks: 'modpack',
  Mods: 'mod',
  DataPacks: 'datapack',
  'Resource Packs': 'resourcepack',
  Shaders: 'shader',
  Worlds: 'world',
  Addons: 'plugin',
  Customizacion: 'mod',
}
const categoryToClassId: Partial<Record<Category, number>> = { Modpacks: 4471, Mods: 6, 'Resource Packs': 12, Worlds: 17, Shaders: 6552, Addons: 4559 }
const numberFormatter = new Intl.NumberFormat('es-ES')
const officialVersions = ['1.21.4', '1.21.3', '1.21.1', '1.21', '1.20.6', '1.20.4', '1.20.1', '1.19.4', '1.18.2', '1.16.5']

const mapModrinthSort = (sort: SortMode) => sort === 'Popularidad' ? 'follows' : sort === 'Ultima Actualizacion' ? 'updated' : sort === 'Mas Descargas' ? 'downloads' : sort === 'Nombre' ? 'newest' : 'relevance'
const mapCurseSortField = (sort: SortMode) => sort === 'Popularidad' ? 2 : sort === 'Ultima Actualizacion' ? 3 : sort === 'Mas Descargas' ? 6 : sort === 'Nombre' ? 4 : sort === 'Actualizacion Estable' ? 11 : 1

export function ExplorerPage() {
  const [category, setCategory] = useState<Category>('All')
  const [sort, setSort] = useState<SortMode>('Relevancia')
  const [view, setView] = useState<ViewMode>('tablero')
  const [platform, setPlatform] = useState<Platform>('Todas')
  const [mcVersion, setMcVersion] = useState('')
  const [loader, setLoader] = useState<LoaderFilter>('Todos')
  const [search, setSearch] = useState('')
  const [items, setItems] = useState<ExplorerItem[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')

  useEffect(() => {
    let cancelled = false
    const fetchData = async () => {
      setLoading(true)
      setError('')
      try {
        const payload = await invoke<ExplorerItem[]>('search_catalogs', {
          request: {
            search,
            category: categoryToProjectType[category],
            curseforgeClassId: categoryToClassId[category] ?? null,
            platform,
            mcVersion: mcVersion || null,
            loader: loader === 'Todos' ? null : loader.toLowerCase(),
            modrinthSort: mapModrinthSort(sort),
            curseforgeSortField: mapCurseSortField(sort),
            limit: 30,
          },
        })
        if (!cancelled) setItems(sortItems(payload, sort))
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err))
      } finally {
        if (!cancelled) setLoading(false)
      }
    }
    void fetchData()
    return () => { cancelled = true }
  }, [search, category, sort, platform, mcVersion, loader])

  const visibleItems = useMemo(() => items.filter((item) => {
    const mcOk = !mcVersion || item.minecraftVersions.some((v) => v.includes(mcVersion))
    const loaderOk = loader === 'Todos' || item.loaders.some((l) => l.toLowerCase().includes(loader.toLowerCase()))
    return mcOk && loaderOk
  }), [items, loader, mcVersion])

  return (
    <main className="content content-padded">
      <section className="instances-panel huge-panel explorer-page">
        <header className="panel-actions explorer-actions-compact">
          <input className="instance-search-compact" placeholder="Buscar en catálogo" value={search} onChange={(e) => setSearch(e.target.value)} />
          <label>Categorías
            <select value={category} onChange={(e) => setCategory(e.target.value as Category)}>{Object.keys(categoryToProjectType).map((value) => <option key={value} value={value}>{value}</option>)}</select>
          </label>
          <label>Orden
            <select value={sort} onChange={(e) => setSort(e.target.value as SortMode)}>{['Relevancia', 'Popularidad', 'Ultima Actualizacion', 'Actualizacion Estable', 'Mas Descargas', 'Nombre', 'Autor'].map((value) => <option key={value} value={value}>{value}</option>)}</select>
          </label>
          <label>Plataforma
            <select value={platform} onChange={(e) => setPlatform(e.target.value as Platform)}>{['Todas', 'Curseforge', 'Modrinth'].map((value) => <option key={value} value={value}>{value}</option>)}</select>
          </label>
          <label>Vista
            <select value={view} onChange={(e) => setView(e.target.value as ViewMode)}>{['lista', 'tablero', 'titulos'].map((value) => <option key={value} value={value}>{value}</option>)}</select>
          </label>
          <details className="advanced-filter-menu">
            <summary>Filtro Avanzado</summary>
            <div className="advanced-filter-body">
              <label>Versión Minecraft
                <select value={mcVersion} onChange={(e) => setMcVersion(e.target.value)}>
                  <option value="">Todas</option>
                  {officialVersions.map((version) => <option key={version} value={version}>{version}</option>)}
                </select>
              </label>
              <label>Loader
                <select value={loader} onChange={(e) => setLoader(e.target.value as LoaderFilter)}>
                  {['Todos', 'Fabric', 'Forge', 'Neoforge', 'Quilt'].map((value) => <option key={value} value={value}>{value}</option>)}
                </select>
              </label>
            </div>
          </details>
        </header>

        <div className="catalog-panel-header">
          <strong>Catálogo completo de CurseForge y Modrinth</strong>
          <small>Conectado al backend con rutas reales y filtros avanzados.</small>
        </div>

        {loading && <p>Cargando catálogo...</p>}
        {error && <p className="error-banner">{error}</p>}

        <div className={`explorer-results ${view}`}>
          {visibleItems.map((item) => (
            <article key={`${item.source}-${item.id}`} className="instance-card explorer-card">
              <div className="instance-card-icon hero" style={item.image ? { backgroundImage: `url(${item.image})` } : undefined} />
              <strong className="instance-card-title">{item.title}</strong>
              {view !== 'titulos' && (
                <>
                  <small>{item.description}</small>
                  <div className="instance-card-meta">
                    <small>{item.source}</small><small>Autor: {item.author}</small><small>Actualizado: {item.updatedAt}</small><small>Descargas: {numberFormatter.format(item.downloads)}</small>
                  </div>
                </>
              )}
            </article>
          ))}
        </div>
      </section>
    </main>
  )
}

function sortItems(items: ExplorerItem[], sort: SortMode): ExplorerItem[] {
  const next = [...items]
  if (sort === 'Mas Descargas' || sort === 'Popularidad') return next.sort((a, b) => b.downloads - a.downloads)
  if (sort === 'Ultima Actualizacion' || sort === 'Actualizacion Estable') return next.sort((a, b) => +new Date(b.updatedAt) - +new Date(a.updatedAt))
  if (sort === 'Nombre') return next.sort((a, b) => a.title.localeCompare(b.title, 'es'))
  if (sort === 'Autor') return next.sort((a, b) => a.author.localeCompare(b.author, 'es'))
  return next
}
