import { invoke } from '@tauri-apps/api/core'
import { Fragment, type ReactNode, useDeferredValue, useEffect, useMemo, useRef, useState } from 'react'

type Category = 'all' | 'modpacks' | 'mods' | 'datapacks' | 'resourcepacks' | 'shaders' | 'worlds' | 'addons' | 'customization'
type SortMode = 'relevance' | 'popularity' | 'updated' | 'stable' | 'downloads' | 'name' | 'author'
type ViewMode = 'list' | 'grid' | 'titles'
type Platform = 'all' | 'curseforge' | 'modrinth'
type LoaderFilter = 'all' | 'fabric' | 'forge' | 'neoforge' | 'quilt'
type TagFilter = 'all' | 'mobs' | 'worldgen' | 'server-utility' | 'technology' | 'adventure' | 'optimization'
type DetailTab = 'description' | 'changelog' | 'gallery' | 'versions' | 'comments'

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

type CatalogVersion = {
  versionType: string
  name: string
  publishedAt: string
  modLoader: string
  gameVersion: string
  downloadUrl: string
  fileUrl: string
}

type CatalogDetail = {
  id: string
  source: 'CurseForge' | 'Modrinth'
  title: string
  author: string
  description: string
  bodyHtml: string
  changelogHtml: string
  url: string
  image: string
  gallery: string[]
  versions: CatalogVersion[]
  commentsUrl: string
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
const officialVersions = [
  '1.21.4', '1.21.3', '1.21.2', '1.21.1', '1.21',
  '1.20.6', '1.20.5', '1.20.4', '1.20.3', '1.20.2', '1.20.1', '1.20',
  '1.19.4', '1.19.3', '1.19.2', '1.19.1', '1.19',
  '1.18.2', '1.18.1', '1.18',
  '1.17.1', '1.17',
  '1.16.5', '1.16.4', '1.16.3', '1.16.2', '1.16.1', '1.16',
  '1.15.2', '1.15.1', '1.15',
  '1.14.4', '1.14.3', '1.14.2', '1.14.1', '1.14',
  '1.13.2', '1.13.1', '1.13',
  '1.12.2', '1.12.1', '1.12',
  '1.11.2', '1.11.1', '1.11',
  '1.10.2',
  '1.9.4', '1.9.3', '1.9.2', '1.9.1', '1.9',
  '1.8.9', '1.8.8', '1.8.7', '1.8.6', '1.8.5', '1.8.4', '1.8.3', '1.8.2', '1.8.1', '1.8',
  '1.7.10', '1.7.9', '1.7.8', '1.7.7', '1.7.6', '1.7.5', '1.7.4', '1.7.2',
  '1.6.4', '1.6.2', '1.6.1',
  '1.5.2', '1.5.1',
  '1.4.7', '1.4.6', '1.4.5', '1.4.4', '1.4.2',
  '1.3.2', '1.3.1',
  '1.2.5', '1.2.4', '1.2.3', '1.2.2', '1.2.1',
  '1.1', '1.0'
]
const PAGE_SIZE = 24
const explorerViewModeKey = 'launcher_explorer_view_mode_v1'
const explorerSidebarWidthKey = 'launcher_explorer_sidebar_width_v1'
const minSidebarWidth = 240
const maxSidebarWidth = 460
const advancedTags: { value: TagFilter; label: string }[] = [
  { value: 'all', label: 'Todos' },
  { value: 'mobs', label: 'Mobs' },
  { value: 'worldgen', label: 'World Gen' },
  { value: 'server-utility', label: 'Server Utility' },
  { value: 'technology', label: 'Technology' },
  { value: 'adventure', label: 'Adventure' },
  { value: 'optimization', label: 'Optimization' },
]

const mapModrinthSort = (sort: SortMode) => sort === 'popularity' ? 'follows' : sort === 'updated' ? 'updated' : sort === 'downloads' ? 'downloads' : sort === 'name' ? 'newest' : 'relevance'
const mapCurseSortField = (sort: SortMode) => sort === 'popularity' ? 2 : sort === 'updated' ? 3 : sort === 'downloads' ? 6 : sort === 'name' ? 4 : sort === 'stable' ? 11 : 1

const uiText = {
  es: { search: 'Buscar en catálogo', categories: 'Categorías', sort: 'Orden', platform: 'Plataforma', view: 'Vista', advanced: 'Filtro avanzado', hideAdvanced: 'Ocultar filtros', mcVersion: 'Versión Minecraft', loader: 'Loader', tags: 'Tags', all: 'Todas', loading: 'Cargando catálogo...', author: 'Autor', downloads: 'Descargas', noResults: 'No hay resultados para los filtros actuales.', page: 'Página', previous: 'Anterior', next: 'Siguiente', relevance: 'Relevancia', popularity: 'Popularidad', updated: 'Actualizado', stable: 'Estable', byDownloads: 'Descargas', byName: 'Nombre', byAuthor: 'Autor', list: 'Lista', grid: 'Cuadrícula', titles: 'Compacto', backToCatalog: 'Volver al catálogo', description: 'Descripción', changelog: 'Changelog', gallery: 'Galería', versions: 'Versiones', comments: 'Comentarios', openSource: 'Abrir página original', noGallery: 'Sin imágenes de galería', noVersions: 'No hay versiones disponibles', commentsHint: 'Comentarios y soporte del proyecto en:', type: 'Type', name: 'Name', date: 'Fecha', modLoaderCol: 'ModLoader', version: 'Version', actions: 'Acciones', install: 'Instalar', lastUpdate: 'Última actualización', compatibleInstances: 'Instancias compatibles', installInInstances: 'Instalar en instancias', dependencies: 'Dependencias obligatorias', close: 'Cerrar', resetFilters: 'Reset filtros' },
  en: { search: 'Search catalog', categories: 'Categories', sort: 'Sort', platform: 'Platform', view: 'View', advanced: 'Advanced filter', hideAdvanced: 'Hide filters', mcVersion: 'Minecraft version', loader: 'Loader', tags: 'Tags', all: 'All', loading: 'Loading catalog...', author: 'Author', downloads: 'Downloads', noResults: 'No results for current filters.', page: 'Page', previous: 'Previous', next: 'Next', relevance: 'Relevance', popularity: 'Popularity', updated: 'Updated', stable: 'Stable', byDownloads: 'Downloads', byName: 'Name', byAuthor: 'Author', list: 'List', grid: 'Grid', titles: 'Compact', backToCatalog: 'Back to catalog', description: 'Description', changelog: 'Changelog', gallery: 'Gallery', versions: 'Versions', comments: 'Comments', openSource: 'Open source page', noGallery: 'No gallery images available', noVersions: 'No versions available', commentsHint: 'Project comments/support available at:', type: 'Type', name: 'Name', date: 'Date', modLoaderCol: 'ModLoader', version: 'Version', actions: 'Actions', install: 'Install', lastUpdate: 'Last update', compatibleInstances: 'Compatible instances', installInInstances: 'Install in instances', dependencies: 'Required dependencies', close: 'Close', resetFilters: 'Reset filters' },
  pt: { search: 'Buscar no catálogo', categories: 'Categorias', sort: 'Ordenar', platform: 'Plataforma', view: 'Visualização', advanced: 'Filtro avançado', hideAdvanced: 'Ocultar filtros', mcVersion: 'Versão do Minecraft', loader: 'Loader', tags: 'Tags', all: 'Todas', loading: 'Carregando catálogo...', author: 'Autor', downloads: 'Downloads', noResults: 'Nenhum resultado para os filtros atuais.', page: 'Página', previous: 'Anterior', next: 'Próxima', relevance: 'Relevância', popularity: 'Popularidade', updated: 'Atualizado', stable: 'Estável', byDownloads: 'Downloads', byName: 'Nome', byAuthor: 'Autor', list: 'Lista', grid: 'Grade', titles: 'Compacto', backToCatalog: 'Voltar ao catálogo', description: 'Descrição', changelog: 'Changelog', gallery: 'Galeria', versions: 'Versões', comments: 'Comentários', openSource: 'Abrir página original', noGallery: 'Sem imagens na galeria', noVersions: 'Sem versões disponíveis', commentsHint: 'Comentários/suporte do projeto em:', type: 'Type', name: 'Name', date: 'Data', modLoaderCol: 'ModLoader', version: 'Version', actions: 'Ações', install: 'Instalar', lastUpdate: 'Última atualização', compatibleInstances: 'Instâncias compatíveis', installInInstances: 'Instalar em instâncias', dependencies: 'Dependências obrigatórias', close: 'Fechar', resetFilters: 'Resetar filtros' },
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

function compactNumber(value: number, uiLanguage: 'es' | 'en' | 'pt') {
  const locale = uiLanguage === 'en' ? 'en-US' : uiLanguage === 'pt' ? 'pt-BR' : 'es-ES'
  return new Intl.NumberFormat(locale, { notation: 'compact', maximumFractionDigits: 1 }).format(value)
}

function cleanLoaderLabel(value: string) {
  const normalized = value.trim().toLowerCase()
  if (!normalized) return '-'
  if (normalized === 'minecraft') return 'Vanilla'
  if (normalized === 'neoforge') return 'NeoForge'
  if (normalized === 'mrpack') return 'Modpack'
  return normalized.charAt(0).toUpperCase() + normalized.slice(1)
}

function parseInlineMarkdown(value: string): ReactNode[] {
  const tokens = value.split(/(!?\[[^\]]*\]\([^)]+\)|\*\*[^*]+\*\*|__[^_]+__|\*[^*]+\*|_[^_]+_)/g).filter(Boolean)
  return tokens.map((token, index) => {
    const image = token.match(/^!\[([^\]]*)\]\((https?:\/\/[^)]+)\)$/)
    if (image) return <img key={`img-${index}`} src={image[2]} alt={image[1]} loading="lazy" referrerPolicy="no-referrer" />

    const link = token.match(/^\[([^\]]+)\]\((https?:\/\/[^)]+)\)$/)
    if (link) return <a key={`link-${index}`} href={link[2]} target="_blank" rel="noreferrer">{link[1]}</a>

    if ((token.startsWith('**') && token.endsWith('**')) || (token.startsWith('__') && token.endsWith('__'))) {
      return <strong key={`strong-${index}`}>{token.slice(2, -2)}</strong>
    }
    if ((token.startsWith('*') && token.endsWith('*')) || (token.startsWith('_') && token.endsWith('_'))) {
      return <em key={`em-${index}`}>{token.slice(1, -1)}</em>
    }
    return <Fragment key={`txt-${index}`}>{token}</Fragment>
  })
}

function renderMarkdown(content: string) {
  const normalized = content.replace(/\r\n/g, '\n').trim()
  if (!normalized) return <p>-</p>

  return normalized.split(/\n{2,}/).map((block, blockIndex) => {
    const line = block.trim()
    if (line.startsWith('### ')) return <h3 key={`h3-${blockIndex}`}>{parseInlineMarkdown(line.slice(4))}</h3>
    if (line.startsWith('## ')) return <h2 key={`h2-${blockIndex}`}>{parseInlineMarkdown(line.slice(3))}</h2>
    if (line.startsWith('# ')) return <h1 key={`h1-${blockIndex}`}>{parseInlineMarkdown(line.slice(2))}</h1>

    const allLines = line.split('\n').map((entry) => entry.trim()).filter(Boolean)
    if (allLines.length > 0 && allLines.every((entry) => /^[-*]\s+/.test(entry))) {
      return <ul key={`ul-${blockIndex}`}>{allLines.map((item, itemIndex) => <li key={`li-${blockIndex}-${itemIndex}`}>{parseInlineMarkdown(item.replace(/^[-*]\s+/, ''))}</li>)}</ul>
    }

    return <p key={`p-${blockIndex}`}>{parseInlineMarkdown(line)}</p>
  })
}

function resolveCardImage(image: string, title: string) {
  if (image) return <img className="instance-card-media" src={image} alt={title} loading="lazy" referrerPolicy="no-referrer" />
  return <div className="explorer-image-fallback" aria-hidden="true">🧩</div>
}

export function ExplorerPage({ uiLanguage }: Props) {
  const t = uiText[uiLanguage]
  const [category, setCategory] = useState<Category>('all')
  const [sort, setSort] = useState<SortMode>('relevance')
  const [view, setView] = useState<ViewMode>(() => (localStorage.getItem(explorerViewModeKey) as ViewMode) || 'grid')
  const [platform, setPlatform] = useState<Platform>('all')
  const [mcVersion, setMcVersion] = useState('')
  const [loader, setLoader] = useState<LoaderFilter>('all')
  const [tag, setTag] = useState<TagFilter>('all')
  const [search, setSearch] = useState('')
  const deferredSearch = useDeferredValue(search)
  const [debouncedSearch, setDebouncedSearch] = useState('')
  const [items, setItems] = useState<ExplorerItem[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const [page, setPage] = useState(1)
  const [showAdvanced, setShowAdvanced] = useState(false)
  const [hasMore, setHasMore] = useState(false)
  const [sidebarWidth, setSidebarWidth] = useState<number>(() => Number(localStorage.getItem(explorerSidebarWidthKey)) || 260)
  const [selectedItem, setSelectedItem] = useState<ExplorerItem | null>(null)
  const [activeTab, setActiveTab] = useState<DetailTab>('description')
  const [detailLoading, setDetailLoading] = useState(false)
  const [detailError, setDetailError] = useState('')
  const [selectedDetail, setSelectedDetail] = useState<CatalogDetail | null>(null)
  const [zoomedImage, setZoomedImage] = useState<string | null>(null)
  const [versionSearch, setVersionSearch] = useState('')
  const [versionFilterOpen, setVersionFilterOpen] = useState(false)
  const [versionMcFilter, setVersionMcFilter] = useState('all')
  const [versionLoaderFilter, setVersionLoaderFilter] = useState('all')
  const [versionPage, setVersionPage] = useState(1)
  const [installModalOpen, setInstallModalOpen] = useState(false)
  const [installQuery, setInstallQuery] = useState('')
  const [selectedVersionForInstall, setSelectedVersionForInstall] = useState<CatalogVersion | null>(null)
  const deferredVersionSearch = useDeferredValue(versionSearch)
  const deferredInstallQuery = useDeferredValue(installQuery)
  const cacheRef = useRef<Record<string, CatalogSearchResponse>>({})
  const detailCacheRef = useRef<Record<string, CatalogDetail>>({})
  const requestSeq = useRef(0)

  useEffect(() => {
    localStorage.setItem(explorerViewModeKey, view)
  }, [view])

  useEffect(() => {
    localStorage.setItem(explorerSidebarWidthKey, String(sidebarWidth))
  }, [sidebarWidth])

  useEffect(() => {
    const timer = window.setTimeout(() => { setDebouncedSearch(deferredSearch.trim()); setPage(1) }, 220)
    return () => window.clearTimeout(timer)
  }, [deferredSearch])

  useEffect(() => {
    const queryKey = JSON.stringify({ debouncedSearch, category, sort, platform, mcVersion, loader, tag, page })
    const cached = cacheRef.current[queryKey]
    if (cached) {
      setItems(cached.items)
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
        tag: tag === 'all' ? null : tag,
        modrinthSort: mapModrinthSort(sort),
        curseforgeSortField: mapCurseSortField(sort),
        limit: PAGE_SIZE,
        page,
      },
    }).then((payload) => {
      if (requestSeq.current !== currentRequest) return
      cacheRef.current[queryKey] = payload
      setItems(payload.items)
      setHasMore(payload.hasMore)
      if (payload.hasMore) {
        const nextPageKey = JSON.stringify({ debouncedSearch, category, sort, platform, mcVersion, loader, tag, page: page + 1 })
        if (!cacheRef.current[nextPageKey]) {
          void invoke<CatalogSearchResponse>('search_catalogs', {
            request: {
              search: debouncedSearch,
              category: categoryToProjectType[category],
              curseforgeClassId: categoryToClassId[category] ?? null,
              platform: platform === 'all' ? 'Todas' : platform === 'curseforge' ? 'Curseforge' : 'Modrinth',
              mcVersion: mcVersion || null,
              loader: loader === 'all' ? null : loader.toLowerCase(),
              tag: tag === 'all' ? null : tag,
              modrinthSort: mapModrinthSort(sort),
              curseforgeSortField: mapCurseSortField(sort),
              limit: PAGE_SIZE,
              page: page + 1,
            },
          }).then((nextPayload) => {
            cacheRef.current[nextPageKey] = nextPayload
          }).catch(() => undefined)
        }
      }
    }).catch((err) => {
      if (requestSeq.current !== currentRequest) return
      setError(err instanceof Error ? err.message : String(err))
    }).finally(() => {
      if (requestSeq.current === currentRequest) setLoading(false)
    })
  }, [debouncedSearch, category, sort, platform, mcVersion, loader, tag, page])

  const handleSidebarResizeStart = (event: React.MouseEvent<HTMLDivElement>) => {
    event.preventDefault()
    const startX = event.clientX
    const startWidth = sidebarWidth
    const onMove = (moveEvent: MouseEvent) => {
      const next = Math.min(maxSidebarWidth, Math.max(minSidebarWidth, startWidth + (moveEvent.clientX - startX)))
      setSidebarWidth(next)
    }
    const onStop = () => {
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onStop)
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onStop)
  }

  useEffect(() => {
    if (!selectedItem) return
    const key = `${selectedItem.source}-${selectedItem.id}`
    const cached = detailCacheRef.current[key]
    if (cached) {
      setSelectedDetail(cached)
      setDetailError('')
      return
    }

    setDetailLoading(true)
    setDetailError('')
    void invoke<CatalogDetail>('get_catalog_detail', {
      request: {
        id: selectedItem.id,
        source: selectedItem.source,
      },
    }).then((payload) => {
      detailCacheRef.current[key] = payload
      setSelectedDetail(payload)
    }).catch((err) => {
      setDetailError(err instanceof Error ? err.message : String(err))
    }).finally(() => setDetailLoading(false))
  }, [selectedItem])

  const dateFormatter = useMemo(() => new Intl.DateTimeFormat(uiLanguage === 'en' ? 'en-US' : uiLanguage === 'pt' ? 'pt-BR' : 'es-ES', { dateStyle: 'medium' }), [uiLanguage])

  const compatibleInstances = useMemo(() => [
    { id: 'inst-1', name: 'Survival Fabric 1.20.1', mc: '1.20.1', loader: 'fabric', compatible: true, hasDependency: false },
    { id: 'inst-2', name: 'Forge RPG 1.19.4', mc: '1.19.4', loader: 'forge', compatible: false, hasDependency: true },
    { id: 'inst-3', name: 'NeoForge Tech 1.21.1', mc: '1.21.1', loader: 'neoforge', compatible: true, hasDependency: true },
  ], [])

  const filteredCompatibleInstances = useMemo(() => {
    const term = deferredInstallQuery.trim().toLowerCase()
    return compatibleInstances.filter((entry) => !term || entry.name.toLowerCase().includes(term))
  }, [compatibleInstances, deferredInstallQuery])

  const filteredVersions = useMemo(() => {
    const list = selectedDetail?.versions ?? []
    return list.filter((version) => {
      const searchTerm = deferredVersionSearch.trim().toLowerCase()
      const bySearch = !searchTerm || version.name.toLowerCase().includes(searchTerm)
      const byMc = versionMcFilter === 'all' || version.gameVersion.toLowerCase().includes(versionMcFilter.toLowerCase())
      const byLoader = versionLoaderFilter === 'all' || version.modLoader.toLowerCase().includes(versionLoaderFilter.toLowerCase())
      return bySearch && byMc && byLoader
    })
  }, [deferredVersionSearch, selectedDetail?.versions, versionLoaderFilter, versionMcFilter])
  const versionsPerPage = 8
  const totalVersionPages = Math.max(1, Math.ceil(filteredVersions.length / versionsPerPage))
  const pagedVersions = filteredVersions.slice((versionPage - 1) * versionsPerPage, versionPage * versionsPerPage)

  return (
    <main className="content content-padded">
      <section className="instances-panel huge-panel explorer-page">
        {!selectedItem && (
          <>
            <div className="explorer-workspace" style={{ gridTemplateColumns: `minmax(${minSidebarWidth}px, ${sidebarWidth}px) 8px minmax(0, 1fr)` }}>
              <aside className="explorer-left-sidebar">
                <div className="explorer-sidebar-section">
                  <input className="instance-search-compact" placeholder={t.search} value={search} onChange={(e) => setSearch(e.target.value)} />
                </div>
                <div className="explorer-sidebar-section">
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
                </div>
                <div className="explorer-sidebar-section explorer-filter-actions">
                  <button className="secondary square" onClick={() => setShowAdvanced((v) => !v)}>{showAdvanced ? t.hideAdvanced : t.advanced}</button>
                  <button className="secondary square explorer-reset-btn" onClick={() => { setSearch(''); setCategory('all'); setSort('relevance'); setPlatform('all'); setMcVersion(''); setLoader('all'); setTag('all'); setPage(1) }}>{t.resetFilters} 🧻</button>
                </div>
                {showAdvanced && (
                  <div className="explorer-sidebar-section">
                    <label>{t.mcVersion}
                      <select value={mcVersion} onChange={(e) => { setMcVersion(e.target.value); setPage(1) }}>
                        <option value="">{t.all}</option>
                        {officialVersions.map((version) => <option key={version} value={version}>{version}</option>)}</select>
                    </label>
                    <label>{t.loader}
                      <select value={loader} onChange={(e) => { setLoader(e.target.value as LoaderFilter); setPage(1) }}>
                        <option value="all">{t.all}</option><option value="fabric">Fabric</option><option value="forge">Forge</option><option value="neoforge">NeoForge</option><option value="quilt">Quilt</option></select>
                    </label>
                    <label>{t.tags}
                      <select value={tag} onChange={(e) => { setTag(e.target.value as TagFilter); setPage(1) }}>
                        {advancedTags.map((entry) => <option key={entry.value} value={entry.value}>{entry.label}</option>)}
                      </select>
                    </label>
                  </div>
                )}
              </aside>
              <div className="explorer-sidebar-resizer" role="separator" aria-orientation="vertical" onMouseDown={handleSidebarResizeStart} />
              <div className="explorer-main-content">

            {loading && <p className="catalog-loader">{t.loading}</p>}
            {error && <p className="error-banner">{error}</p>}

            <div className={`explorer-results ${view}`}>
              {items.map((item) => (
                <article key={`${item.source}-${item.id}`} className="instance-card explorer-card clickable" onClick={() => { setSelectedItem(item); setSelectedDetail(null); setActiveTab('description') }}>
                  <div className="explorer-card-media-wrapper">
                    <div className="instance-card-icon hero explorer-card-media">
                      {resolveCardImage(item.image, item.title)}
                    </div>
                  </div>
                  <div className="explorer-card-info">
                    <strong className="instance-card-title" title={item.title}>{item.title}</strong>
                    {view !== 'titles' && (
                      <>
                        <small className="explorer-description" title={item.description}>{item.description}</small>
                        <div className="explorer-top-badges">
                          <span className={`platform-badge ${item.source.toLowerCase()}`}>{item.source}</span>
                          <span className="loader-badge">{cleanLoaderLabel(item.loaders[0] ?? item.projectType)}</span>
                          {item.minecraftVersions[0] ? <span className="mc-chip">MC {item.minecraftVersions[0]}</span> : null}
                        </div>
                        <div className="instance-card-meta explorer-meta-grid">
                          <small>{t.author}: {item.author}</small>
                          <small>{t.downloads}: {compactNumber(item.downloads, uiLanguage)}</small>
                        <small>{t.lastUpdate}: {item.updatedAt ? dateFormatter.format(new Date(item.updatedAt)) : '-'}</small>
                          {item.updatedAt ? <small>{dateFormatter.format(new Date(item.updatedAt))}</small> : null}
                        </div>
                        <div className="explorer-tags">{item.tags.slice(0, 3).map((tag) => <span key={tag}>{cleanLoaderLabel(tag)}</span>)}</div>
                      </>
                    )}
                  </div>
                </article>
              ))}
            </div>

            {!loading && items.length === 0 ? <p>{t.noResults}</p> : null}

            <footer className="explorer-pagination explorer-pagination-bar">
              <button className="secondary" onClick={() => setPage((p) => Math.max(1, p - 1))} disabled={page <= 1 || loading}>{t.previous}</button>
              <span>{t.page} {page}</span>
              <button className="secondary" onClick={() => setPage((p) => p + 1)} disabled={loading || !hasMore}>{t.next}</button>
            </footer>
              </div>
            </div>
          </>
        )}

        {selectedItem && (
          <div className="explorer-detail-layout">
            <button className="secondary" onClick={() => setSelectedItem(null)}>{t.backToCatalog}</button>
            <article className="instance-card explorer-detail-hero">
              <div className="explorer-card-media-wrapper">
                <div className="instance-card-icon hero explorer-card-media">
                  {resolveCardImage(selectedDetail?.image || selectedItem.image, selectedItem.title)}
                </div>
              </div>
              <div className="explorer-card-info">
                <strong className="instance-card-title">{selectedDetail?.title || selectedItem.title}</strong>
                <small className="explorer-description">{selectedDetail?.description || selectedItem.description}</small>
                <div className="explorer-top-badges">
                  <span className={`platform-badge ${selectedItem.source.toLowerCase()}`}>{selectedItem.source}</span>
                  <span className="loader-badge">{cleanLoaderLabel(selectedItem.loaders[0] ?? selectedItem.projectType)}</span>
                  {selectedItem.minecraftVersions[0] ? <span className="mc-chip">MC {selectedItem.minecraftVersions[0]}</span> : null}
                </div>
                <div className="explorer-detail-stats">
                  <small>{t.author}: {selectedDetail?.author || selectedItem.author}</small>
                  <small>{t.downloads}: {compactNumber(selectedItem.downloads, uiLanguage)}</small>
                  <small>Tamaño: {selectedItem.size || '-'}</small>
                  <small>Loader: {selectedItem.loaders.map((entry) => cleanLoaderLabel(entry)).join(', ') || cleanLoaderLabel(selectedItem.projectType)}</small>
                  <small>MC: {selectedItem.minecraftVersions[0] || '-'}</small>
                  <small>Versión complemento: {selectedDetail?.versions[0]?.name || '-'}</small>
                  <small>{t.lastUpdate}: {selectedItem.updatedAt ? dateFormatter.format(new Date(selectedItem.updatedAt)) : '-'}</small>
                </div>
                {!!selectedDetail?.url && <a className="secondary explorer-link" href={selectedDetail.url} target="_blank" rel="noreferrer">{t.openSource}</a>}
              </div>
            </article>

            <div className="explorer-detail-tabs">
              {(['description', 'changelog', 'gallery', 'versions', 'comments'] as const).map((tab) => (
                <button key={tab} className={activeTab === tab ? 'active' : ''} onClick={() => setActiveTab(tab)}>{t[tab]}</button>
              ))}
            </div>

            {detailLoading && <p className="catalog-loader">{t.loading}</p>}
            {detailError && <p className="error-banner">{detailError}</p>}

            {!!selectedDetail && (
              <div className="explorer-detail-panel">
                {activeTab === 'description' && (
                  <div className="explorer-detail-html">
                    {renderMarkdown(selectedDetail.bodyHtml || selectedDetail.description || selectedItem.description)}
                  </div>
                )}
                {activeTab === 'changelog' && (
                  <div className="explorer-detail-html">
                    <div>{renderMarkdown(selectedDetail.changelogHtml || selectedDetail.description)}<div className="explorer-changelog-cards">{selectedDetail.versions.slice(0, 10).map((version, idx) => <article key={`${version.name}-changelog-${idx}`}><strong>{version.name}</strong><small>{version.publishedAt ? dateFormatter.format(new Date(version.publishedAt)) : '-'}</small><p>{version.versionType} · {version.modLoader || '-'} · MC {version.gameVersion || '-'}</p></article>)}</div></div>
                  </div>
                )}
                {activeTab === 'gallery' && (
                  <div className="explorer-gallery-grid">
                    {selectedDetail.gallery.length === 0 && <p>{t.noGallery}</p>}
                    {selectedDetail.gallery.map((image) => <img key={image} src={image} loading="lazy" referrerPolicy="no-referrer" alt={selectedDetail.title} onClick={() => setZoomedImage((prev) => prev === image ? null : image)} />)}
                  </div>
                )}
                {activeTab === 'versions' && (
                  <div className="explorer-versions-wrap">
                    <div className="explorer-actions-compact">
                      <input type="search" value={versionSearch} onChange={(event) => { setVersionSearch(event.target.value); setVersionPage(1) }} placeholder="Buscar versión" />
                      <button className="ghost-btn" onClick={() => setVersionFilterOpen((prev) => !prev)}>{versionFilterOpen ? t.hideAdvanced : t.advanced}</button>
                    </div>
                    {versionFilterOpen && (
                      <div className="advanced-filter-body inline">
                        <label>{t.mcVersion}
                          <input value={versionMcFilter === 'all' ? '' : versionMcFilter} onChange={(event) => { setVersionMcFilter(event.target.value || 'all'); setVersionPage(1) }} placeholder="1.20.1" />
                        </label>
                        <label>{t.loader}
                          <input value={versionLoaderFilter === 'all' ? '' : versionLoaderFilter} onChange={(event) => { setVersionLoaderFilter(event.target.value || 'all'); setVersionPage(1) }} placeholder="fabric" />
                        </label>
                      </div>
                    )}
                    {filteredVersions.length === 0 && <p>{t.noVersions}</p>}
                    <div className="explorer-version-cards">
                      {pagedVersions.map((version, idx) => (
                        <article key={`${version.name}-${idx}`} className="explorer-version-card">
                          <strong>{version.name}</strong>
                          <small>{version.versionType} · {version.publishedAt ? dateFormatter.format(new Date(version.publishedAt)) : '-'}</small>
                          <p>Loader: {version.modLoader || '-'} · MC {version.gameVersion || '-'}</p>
                          <button className="action-elevated" onClick={() => { setSelectedVersionForInstall(version); setInstallModalOpen(true) }}>{t.install}</button>
                        </article>
                      ))}
                    </div>
                    <footer className="explorer-pagination">
                      <button className="square" onClick={() => setVersionPage((prev) => Math.max(1, prev - 1))} disabled={versionPage <= 1}>{t.previous}</button>
                      <span>{t.page} {versionPage} / {totalVersionPages}</span>
                      <button className="square" onClick={() => setVersionPage((prev) => Math.min(totalVersionPages, prev + 1))} disabled={versionPage >= totalVersionPages}>{t.next}</button>
                    </footer>
                  </div>
                )}
                {activeTab === 'comments' && (
                  <div>
                    <p>{t.commentsHint}</p>
                    <a href={selectedDetail.commentsUrl || selectedDetail.url} target="_blank" rel="noreferrer">{selectedDetail.commentsUrl || selectedDetail.url}</a>
                  </div>
                )}
              </div>
            )}
          </div>
        )}
      </section>

      {zoomedImage && (
        <div className="gallery-zoom-backdrop" onClick={() => setZoomedImage(null)} role="button" tabIndex={0}>
          <img src={zoomedImage} alt="Zoom" className="gallery-zoom-image" />
        </div>
      )}
      {installModalOpen && (
        <div className="floating-modal-overlay" role="dialog" aria-modal="true" aria-label={t.installInInstances}>
          <div className="floating-modal explorer-install-modal">
            <h3>{t.installInInstances}</h3>
            <input type="search" value={installQuery} onChange={(event) => setInstallQuery(event.target.value)} placeholder="Buscar instancia" />
            <div className="explorer-install-list">
              {filteredCompatibleInstances.map((instance) => (
                <label key={instance.id} className={instance.compatible ? '' : 'disabled'}>
                  <input type="checkbox" disabled={!instance.compatible} /> {instance.name} · {instance.mc} · {instance.loader}
                  {instance.hasDependency && <small>{t.dependencies}: ya instalada en esta instancia</small>}
                </label>
              ))}
            </div>
            <p>{t.dependencies}: {selectedVersionForInstall?.name || '-'}</p>
            <div className="modal-actions"><button onClick={() => setInstallModalOpen(false)}>{t.close}</button><button>{t.install}</button></div>
          </div>
        </div>
      )}

    </main>
  )
}
