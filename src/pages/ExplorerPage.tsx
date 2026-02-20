import { invoke } from '@tauri-apps/api/core'
import { useEffect, useMemo, useRef, useState } from 'react'

type Category = 'all' | 'modpacks' | 'mods' | 'datapacks' | 'resourcepacks' | 'shaders' | 'worlds' | 'addons' | 'customization'
type SortMode = 'relevance' | 'popularity' | 'updated' | 'stable' | 'downloads' | 'name' | 'author'
type ViewMode = 'list' | 'grid' | 'titles'
type Platform = 'all' | 'curseforge' | 'modrinth'
type LoaderFilter = 'all' | 'fabric' | 'forge' | 'neoforge' | 'quilt'

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

type CatalogSearchResponse = {
  items: ExplorerItem[]
  page: number
  limit: number
  hasMore: boolean
}

type Props = { uiLanguage: 'es' | 'en' | 'pt' }

const categoryToProjectType: Record<Category, string | null> = {
  all: null,
  modpacks: 'modpack',
  mods: 'mod',
  datapacks: 'datapack',
  resourcepacks: 'resourcepack',
  shaders: 'shader',
  worlds: 'world',
  addons: 'plugin',
  customization: 'mod',
}
const categoryToClassId: Partial<Record<Category, number>> = { modpacks: 4471, mods: 6, resourcepacks: 12, worlds: 17, shaders: 6552, addons: 4559 }
const officialVersions = ['1.21.4', '1.21.3', '1.21.1', '1.21', '1.20.6', '1.20.4', '1.20.1', '1.19.4', '1.18.2', '1.16.5']
const PAGE_SIZE = 24
const explorerViewModeKey = 'launcher_explorer_view_mode_v1'

const mapModrinthSort = (sort: SortMode) => sort === 'popularity' ? 'follows' : sort === 'updated' ? 'updated' : sort === 'downloads' ? 'downloads' : sort === 'name' ? 'newest' : 'relevance'
const mapCurseSortField = (sort: SortMode) => sort === 'popularity' ? 2 : sort === 'updated' ? 3 : sort === 'downloads' ? 6 : sort === 'name' ? 4 : sort === 'stable' ? 11 : 1

const uiText = {
  es: { search: 'Buscar en catálogo', categories: 'Categorías', sort: 'Orden', platform: 'Plataforma', view: 'Vista', advanced: 'Filtro avanzado', hideAdvanced: 'Ocultar filtros', mcVersion: 'Versión Minecraft', loader: 'Loader', all: 'Todas', headerTitle: 'Catálogo completo de CurseForge y Modrinth', headerSub: 'Resultados optimizados con backend, filtros robustos, caché y paginación.', loading: 'Cargando catálogo...', author: 'Autor', downloads: 'Descargas', noResults: 'No hay resultados para los filtros actuales.', page: 'Página', previous: 'Anterior', next: 'Siguiente', relevance: 'Relevancia', popularity: 'Popularidad', updated: 'Actualizado', stable: 'Estable', byDownloads: 'Descargas', byName: 'Nombre', byAuthor: 'Autor', list: 'Lista', grid: 'Cuadrícula', titles: 'Compacto', retry: 'Reintentar' },
  en: { search: 'Search catalog', categories: 'Categories', sort: 'Sort', platform: 'Platform', view: 'View', advanced: 'Advanced filter', hideAdvanced: 'Hide filters', mcVersion: 'Minecraft version', loader: 'Loader', all: 'All', headerTitle: 'Complete CurseForge and Modrinth catalog', headerSub: 'Optimized backend results with robust filters, cache and pagination.', loading: 'Loading catalog...', author: 'Author', downloads: 'Downloads', noResults: 'No results for current filters.', page: 'Page', previous: 'Previous', next: 'Next', relevance: 'Relevance', popularity: 'Popularity', updated: 'Updated', stable: 'Stable', byDownloads: 'Downloads', byName: 'Name', byAuthor: 'Author', list: 'List', grid: 'Grid', titles: 'Compact', retry: 'Retry' },
  pt: { search: 'Buscar no catálogo', categories: 'Categorias', sort: 'Ordenar', platform: 'Plataforma', view: 'Visualização', advanced: 'Filtro avançado', hideAdvanced: 'Ocultar filtros', mcVersion: 'Versão do Minecraft', loader: 'Loader', all: 'Todas', headerTitle: 'Catálogo completo de CurseForge e Modrinth', headerSub: 'Resultados otimizados com backend, filtros robustos, cache e paginação.', loading: 'Carregando catálogo...', author: 'Autor', downloads: 'Downloads', noResults: 'Nenhum resultado para os filtros atuais.', page: 'Página', previous: 'Anterior', next: 'Próxima', relevance: 'Relevância', popularity: 'Popularidade', updated: 'Atualizado', stable: 'Estável', byDownloads: 'Downloads', byName: 'Nome', byAuthor: 'Autor', list: 'Lista', grid: 'Grade', titles: 'Compacto', retry: 'Tentar novamente' },
} as const

const labels = {
  category: {
    all: { es: 'Todas', en: 'All', pt: 'Todas' },
    modpacks: { es: 'Modpacks', en: 'Modpacks', pt: 'Modpacks' },
    mods: { es: 'Mods', en: 'Mods', pt: 'Mods' },
    datapacks: { es: 'Data Packs', en: 'Data Packs', pt: 'Data Packs' },
    resourcepacks: { es: 'Resource Packs', en: 'Resource Packs', pt: 'Resource Packs' },
    shaders: { es: 'Shaders', en: 'Shaders', pt: 'Shaders' },
    worlds: { es: 'Mundos', en: 'Worlds', pt: 'Mundos' },
    addons: { es: 'Addons', en: 'Addons', pt: 'Addons' },
    customization: { es: 'Customización', en: 'Customization', pt: 'Customização' },
  },
} as const

export function ExplorerPage({ uiLanguage }: Props) {
  const t = uiText[uiLanguage]
  const [category, setCategory] = useState<Category>('all')
  const [sort, setSort] = useState<SortMode>('relevance')
  const [view, setView] = useState<ViewMode>(() => (localStorage.getItem(explorerViewModeKey) as ViewMode) || 'grid')
  const [platform, setPlatform] = useState<Platform>('all')
  const [mcVersion, setMcVersion] = useState('')
  const [loader, setLoader] = useState<LoaderFilter>('all')
  const [search, setSearch] = useState('')
  const [debouncedSearch, setDebouncedSearch] = useState('')
  const [items, setItems] = useState<ExplorerItem[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const [page, setPage] = useState(1)
  const [showAdvanced, setShowAdvanced] = useState(false)
  const [hasMore, setHasMore] = useState(false)
  const [reloadTick, setReloadTick] = useState(0)
  const cacheRef = useRef<Record<string, CatalogSearchResponse>>({})
  const requestSeq = useRef(0)

  useEffect(() => {
    localStorage.setItem(explorerViewModeKey, view)
  }, [view])

  useEffect(() => {
    const timer = window.setTimeout(() => { setDebouncedSearch(search.trim()); setPage(1) }, 320)
    return () => window.clearTimeout(timer)
  }, [search])

  useEffect(() => {
    const queryKey = JSON.stringify({ debouncedSearch, category, sort, platform, mcVersion, loader, page })
    const cached = cacheRef.current[queryKey]
    if (cached) {
      setItems(sortItems(cached.items, sort, uiLanguage))
      setHasMore(cached.hasMore)
      setError('')
      return
    }

    const currentRequest = requestSeq.current + 1
    requestSeq.current = currentRequest
    setLoading(true)
    setError('')

    void invoke<CatalogSearchResponse>('search_catalogs', {
      request: {
        search: debouncedSearch,
        category: categoryToProjectType[category],
        curseforgeClassId: categoryToClassId[category] ?? null,
        platform: platform === 'all' ? 'Todas' : platform === 'curseforge' ? 'Curseforge' : 'Modrinth',
        mcVersion: mcVersion || null,
        loader: loader === 'all' ? null : loader.toLowerCase(),
        modrinthSort: mapModrinthSort(sort),
        curseforgeSortField: mapCurseSortField(sort),
        limit: PAGE_SIZE,
        page,
      },
    }).then((payload) => {
      if (requestSeq.current !== currentRequest) return
      cacheRef.current[queryKey] = payload
      setItems(sortItems(payload.items, sort, uiLanguage))
      setHasMore(payload.hasMore)
    }).catch((err) => {
      if (requestSeq.current !== currentRequest) return
      setError(err instanceof Error ? err.message : String(err))
    }).finally(() => {
      if (requestSeq.current === currentRequest) setLoading(false)
    })
  }, [debouncedSearch, category, sort, platform, mcVersion, loader, page, uiLanguage, reloadTick])

  const numberFormatter = useMemo(() => new Intl.NumberFormat(uiLanguage === 'en' ? 'en-US' : uiLanguage === 'pt' ? 'pt-BR' : 'es-ES'), [uiLanguage])

  return (
    <main className="content content-padded">
      <section className="instances-panel huge-panel explorer-page">
        <header className="panel-actions explorer-actions-compact">
          <input className="instance-search-compact" placeholder={t.search} value={search} onChange={(e) => setSearch(e.target.value)} />
          <label>{t.categories}
            <select value={category} onChange={(e) => { setCategory(e.target.value as Category); setPage(1) }}>{Object.keys(categoryToProjectType).map((value) => <option key={value} value={value}>{labels.category[value as Category][uiLanguage]}</option>)}</select>
          </label>
          <label>{t.sort}
            <select value={sort} onChange={(e) => { setSort(e.target.value as SortMode); setPage(1) }}><option value="relevance">{t.relevance}</option><option value="popularity">{t.popularity}</option><option value="updated">{t.updated}</option><option value="stable">{t.stable}</option><option value="downloads">{t.byDownloads}</option><option value="name">{t.byName}</option><option value="author">{t.byAuthor}</option></select>
          </label>
          <label>{t.platform}
            <select value={platform} onChange={(e) => { setPlatform(e.target.value as Platform); setPage(1) }}><option value="all">{t.all}</option><option value="curseforge">CurseForge</option><option value="modrinth">Modrinth</option></select>
          </label>
          <label>{t.view}
            <select value={view} onChange={(e) => setView(e.target.value as ViewMode)}><option value="list">{t.list}</option><option value="grid">{t.grid}</option><option value="titles">{t.titles}</option></select>
          </label>
          <button className="secondary" onClick={() => setShowAdvanced((v) => !v)}>{showAdvanced ? t.hideAdvanced : t.advanced}</button>
          {showAdvanced && (
            <div className="advanced-filter-body inline">
              <label>{t.mcVersion}
                <select value={mcVersion} onChange={(e) => { setMcVersion(e.target.value); setPage(1) }}>
                  <option value="">{t.all}</option>
                  {officialVersions.map((version) => <option key={version} value={version}>{version}</option>)}</select>
              </label>
              <label>{t.loader}
                <select value={loader} onChange={(e) => { setLoader(e.target.value as LoaderFilter); setPage(1) }}>
                  <option value="all">{t.all}</option><option value="fabric">Fabric</option><option value="forge">Forge</option><option value="neoforge">NeoForge</option><option value="quilt">Quilt</option></select>
              </label>
            </div>
          )}
        </header>

        <div className="catalog-panel-header">
          <strong>{t.headerTitle}</strong>
          <small>{t.headerSub}</small>
        </div>

        {loading && <p className="catalog-loader">{t.loading}</p>}
        {error && <p className="error-banner">{error} <button className="secondary" onClick={() => { cacheRef.current = {}; setReloadTick((v) => v + 1) }}>{t.retry}</button></p>}

        <div className={`explorer-results ${view}`}>
          {items.map((item) => (
            <article key={`${item.source}-${item.id}`} className="instance-card explorer-card">
              <div className="explorer-card-media-wrapper">
                <div className="instance-card-icon hero explorer-card-media">
                  {item.image ? <img className="instance-card-media" src={item.image} alt={item.title} loading="lazy" referrerPolicy="no-referrer" /> : null}
                </div>
              </div>
              <div className="explorer-card-info">
                <strong className="instance-card-title" title={item.title}>{item.title}</strong>
                {view !== 'titles' && (
                  <>
                    <small className="explorer-description" title={item.description}>{item.description}</small>
                    <div className="instance-card-meta">
                      <small><span className={`platform-badge ${item.source.toLowerCase()}`}>{item.source}</span></small><small>{t.author}: {item.author}</small><small><span className="loader-badge">{item.loaders[0] ?? item.projectType}</span></small><small>{t.downloads}: {numberFormatter.format(item.downloads)}</small>
                    </div>
                    <div className="explorer-tags">{item.tags.slice(0, 4).map((tag) => <span key={tag}>{tag}</span>)}</div>
                  </>
                )}
              </div>
            </article>
          ))}
        </div>

        {!loading && items.length === 0 ? <p>{t.noResults}</p> : null}

        <footer className="explorer-pagination">
          <button className="secondary" onClick={() => setPage((p) => Math.max(1, p - 1))} disabled={page <= 1 || loading}>{t.previous}</button>
          <span>{t.page} {page}</span>
          <button className="secondary" onClick={() => setPage((p) => p + 1)} disabled={loading || !hasMore}>{t.next}</button>
        </footer>
      </section>
    </main>
  )
}

function sortItems(items: ExplorerItem[], sort: SortMode, uiLanguage: 'es' | 'en' | 'pt'): ExplorerItem[] {
  const next = [...items]
  if (sort === 'downloads' || sort === 'popularity') return next.sort((a, b) => b.downloads - a.downloads)
  if (sort === 'updated' || sort === 'stable') return next.sort((a, b) => +new Date(b.updatedAt) - +new Date(a.updatedAt))
  const locale = uiLanguage === 'en' ? 'en' : uiLanguage === 'pt' ? 'pt' : 'es'
  if (sort === 'name') return next.sort((a, b) => a.title.localeCompare(b.title, locale))
  if (sort === 'author') return next.sort((a, b) => a.author.localeCompare(b.author, locale))
  return next
}
