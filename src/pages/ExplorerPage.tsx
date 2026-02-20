import { useEffect, useMemo, useState } from 'react'

type Category = 'All' | 'Modpacks' | 'Mods' | 'DataPacks' | 'Resource Packs' | 'Shaders' | 'Worlds' | 'Addons' | 'Customizacion'
type SortMode = 'Relevancia' | 'Popularidad' | 'Ultima Actualizacion' | 'Actualizacion Estable' | 'Mas Descargas' | 'Nombre' | 'Autor'
type ViewMode = 'lista' | 'tablero' | 'titulos'
type Platform = 'Todas' | 'Curseforge' | 'Modrinth'

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

const curseforgeApiKey = '$2a$10$jK7YyZHdUNTDlcME9Egd6.Zt5RananLQKn/tpIhmRDezd2.wHGU9G'
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

const categoryToClassId: Partial<Record<Category, number>> = {
  Modpacks: 4471,
  Mods: 6,
  'Resource Packs': 12,
  Worlds: 17,
  Shaders: 6552,
  Addons: 4559,
}

const numberFormatter = new Intl.NumberFormat('es-ES')

const mapModrinthSort = (sort: SortMode) => {
  if (sort === 'Popularidad') return 'follows'
  if (sort === 'Ultima Actualizacion') return 'updated'
  if (sort === 'Mas Descargas') return 'downloads'
  if (sort === 'Nombre') return 'newest'
  return 'relevance'
}

const mapCurseSortField = (sort: SortMode) => {
  if (sort === 'Popularidad') return 2
  if (sort === 'Ultima Actualizacion') return 3
  if (sort === 'Mas Descargas') return 6
  if (sort === 'Nombre') return 4
  if (sort === 'Actualizacion Estable') return 11
  return 1
}

export function ExplorerPage() {
  const [category, setCategory] = useState<Category>('All')
  const [sort, setSort] = useState<SortMode>('Relevancia')
  const [view, setView] = useState<ViewMode>('tablero')
  const [platform, setPlatform] = useState<Platform>('Todas')
  const [mcVersion, setMcVersion] = useState('')
  const [loader, setLoader] = useState('')
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
        const [modrinthItems, curseforgeItems] = await Promise.all([
          platform === 'Curseforge' ? Promise.resolve([]) : fetchModrinth(search, category, sort, mcVersion, loader),
          platform === 'Modrinth' ? Promise.resolve([]) : fetchCurseforge(search, category, sort, mcVersion),
        ])
        if (!cancelled) {
          setItems(sortItems([...modrinthItems, ...curseforgeItems], sort))
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err))
        }
      } finally {
        if (!cancelled) setLoading(false)
      }
    }
    void fetchData()
    return () => {
      cancelled = true
    }
  }, [search, category, sort, platform, mcVersion, loader])

  const visibleItems = useMemo(() => {
    if (!loader && !mcVersion) return items
    return items.filter((item) => {
      const mcOk = !mcVersion || item.minecraftVersions.some((v) => v.includes(mcVersion))
      const loaderOk = !loader || item.loaders.some((l) => l.toLowerCase().includes(loader.toLowerCase()))
      return mcOk && loaderOk
    })
  }, [items, loader, mcVersion])

  return (
    <main className="content content-padded">
      <section className="instances-panel huge-panel explorer-page">
        <header className="panel-actions">
          <input className="instance-search-compact" placeholder="Buscar en catálogo" value={search} onChange={(e) => setSearch(e.target.value)} />
        </header>

        <div className="explorer-toolbar">
          <label>Categorias
            <select value={category} onChange={(e) => setCategory(e.target.value as Category)}>
              {Object.keys(categoryToProjectType).map((value) => <option key={value} value={value}>{value}</option>)}
            </select>
          </label>
          <label>Sort
            <select value={sort} onChange={(e) => setSort(e.target.value as SortMode)}>
              {['Relevancia', 'Popularidad', 'Ultima Actualizacion', 'Actualizacion Estable', 'Mas Descargas', 'Nombre', 'Autor'].map((value) => <option key={value} value={value}>{value}</option>)}
            </select>
          </label>
          <label>Filtro versión
            <input value={mcVersion} onChange={(e) => setMcVersion(e.target.value)} placeholder="1.20.1" />
          </label>
          <label>Loader
            <input value={loader} onChange={(e) => setLoader(e.target.value)} placeholder="forge/fabric" />
          </label>
          <label>Plataforma
            <select value={platform} onChange={(e) => setPlatform(e.target.value as Platform)}>
              {['Todas', 'Curseforge', 'Modrinth'].map((value) => <option key={value} value={value}>{value}</option>)}
            </select>
          </label>
          <label>Vista
            <select value={view} onChange={(e) => setView(e.target.value as ViewMode)}>
              {['lista', 'tablero', 'titulos'].map((value) => <option key={value} value={value}>{value}</option>)}
            </select>
          </label>
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
                    <small>{item.source}</small>
                    <small>Autor: {item.author}</small>
                    <small>Actualizado: {item.updatedAt}</small>
                    <small>Descargas: {numberFormatter.format(item.downloads)}</small>
                    <small>Tamaño: {item.size}</small>
                    <small>MC: {item.minecraftVersions.slice(0, 2).join(', ') || '-'}</small>
                    <small>Loader: {item.loaders.slice(0, 2).join(', ') || '-'}</small>
                    <small>Tipo: {item.projectType}</small>
                    <small>Tags: {item.tags.slice(0, 3).join(', ') || '-'}</small>
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

async function fetchModrinth(search: string, category: Category, sort: SortMode, mcVersion: string, loader: string): Promise<ExplorerItem[]> {
  const projectType = categoryToProjectType[category]
  const facets: string[][] = []
  if (projectType) facets.push([`project_type:${projectType}`])
  if (mcVersion) facets.push([`versions:${mcVersion}`])
  if (loader) facets.push([`categories:${loader.toLowerCase()}`])

  const params = new URLSearchParams({
    query: search,
    limit: '30',
    index: mapModrinthSort(sort),
    facets: JSON.stringify(facets),
  })

  const response = await fetch(`https://api.modrinth.com/v2/search?${params.toString()}`)
  if (!response.ok) throw new Error(`Modrinth respondió con ${response.status}`)
  const payload = await response.json() as { hits: Array<Record<string, unknown>> }
  return payload.hits.map((hit) => {
    const categories = Array.isArray(hit.categories) ? hit.categories.filter((item): item is string => typeof item === 'string') : []
    const versions = Array.isArray(hit.versions) ? hit.versions.filter((item): item is string => typeof item === 'string') : []
    return {
      id: String(hit.project_id ?? crypto.randomUUID()),
      source: 'Modrinth',
      title: String(hit.title ?? 'Sin título'),
      description: String(hit.description ?? ''),
      image: String(hit.icon_url ?? ''),
      author: String(hit.author ?? '-'),
      downloads: Number(hit.downloads ?? 0),
      updatedAt: String(hit.date_modified ?? ''),
      size: '-',
      minecraftVersions: versions,
      loaders: categories,
      projectType: String(hit.project_type ?? '-'),
      tags: categories,
    }
  })
}

async function fetchCurseforge(search: string, category: Category, sort: SortMode, mcVersion: string): Promise<ExplorerItem[]> {
  const params = new URLSearchParams({
    gameId: '432',
    pageSize: '30',
    sortField: String(mapCurseSortField(sort)),
    sortOrder: 'desc',
  })
  if (search) params.set('searchFilter', search)
  if (mcVersion) params.set('gameVersion', mcVersion)
  const classId = categoryToClassId[category]
  if (classId) params.set('classId', String(classId))

  const response = await fetch(`https://api.curseforge.com/v1/mods/search?${params.toString()}`, {
    headers: { 'x-api-key': curseforgeApiKey },
  })
  if (!response.ok) throw new Error(`CurseForge respondió con ${response.status}`)
  const payload = await response.json() as { data: Array<Record<string, unknown>> }
  return payload.data.map((entry) => {
    const latestIndexes = Array.isArray(entry.latestFilesIndexes) ? entry.latestFilesIndexes as Array<Record<string, unknown>> : []
    const gameVersions = latestIndexes.map((item) => String(item.gameVersion ?? '')).filter(Boolean)
    const loaders = latestIndexes.map((item) => String(item.modLoader ?? '')).filter(Boolean)
    return {
      id: String(entry.id ?? crypto.randomUUID()),
      source: 'CurseForge',
      title: String(entry.name ?? 'Sin título'),
      description: String(entry.summary ?? ''),
      image: String((entry.logo as { thumbnailUrl?: string } | null)?.thumbnailUrl ?? ''),
      author: String((entry.authors as Array<{ name?: string }> | undefined)?.[0]?.name ?? '-'),
      downloads: Number(entry.downloadCount ?? 0),
      updatedAt: String(entry.dateReleased ?? ''),
      size: '-',
      minecraftVersions: gameVersions,
      loaders,
      projectType: String((entry.class as { name?: string } | null)?.name ?? '-'),
      tags: Array.isArray(entry.categories) ? (entry.categories as Array<{ name?: string }>).map((item) => item.name ?? '').filter(Boolean) : [],
    }
  })
}

function sortItems(items: ExplorerItem[], sort: SortMode): ExplorerItem[] {
  const next = [...items]
  if (sort === 'Mas Descargas' || sort === 'Popularidad') return next.sort((a, b) => b.downloads - a.downloads)
  if (sort === 'Ultima Actualizacion' || sort === 'Actualizacion Estable') return next.sort((a, b) => +new Date(b.updatedAt) - +new Date(a.updatedAt))
  if (sort === 'Nombre') return next.sort((a, b) => a.title.localeCompare(b.title, 'es'))
  if (sort === 'Autor') return next.sort((a, b) => a.author.localeCompare(b.author, 'es'))
  return next
}
