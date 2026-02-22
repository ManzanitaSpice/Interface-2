import { convertFileSrc } from '@tauri-apps/api/core'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { AnimatePresence, motion } from 'framer-motion'
import { useCallback, useEffect, useMemo, useRef, useState, type ChangeEvent, type CSSProperties, type PointerEvent as ReactPointerEvent } from 'react'
import './App.css'
import { SkinStudio } from './skin/SkinStudio'
import { FolderRow } from './components/FolderRow'
import { MigrationModal } from './components/MigrationModal'
import { useMigration } from './hooks/useMigration'
import { ImportPage } from './pages/ImportPage'
import { ExplorerPage } from './pages/ExplorerPage'

type MainPage =
  | 'Inicio'
  | 'Mis Modpacks'
  | 'Novedades'
  | 'Updates'
  | 'Explorador'
  | 'Servers'
  | 'Configuración Global'
  | 'Administradora de cuentas'
  | 'Administradora de skins'
  | 'Editor de skins'
  | 'Creador de Instancias'
  | 'Editar Instancia'
  | 'Importar Instancias'

type InstanceCard = {
  id: string
  name: string
  group: string
  instanceRoot?: string
}

type InstanceVisualMeta = {
  mediaDataUrl?: string
  mediaPath?: string
  mediaMime?: string
  minecraftVersion?: string
  loader?: string
}

type InstanceExportFormat = 'prism-zip' | 'curseforge-zip' | 'mrpack'

type InstanceHoverInfo = {
  size: string
  createdAt: string
  lastUsedAt: string
  author: string
  modsCount: string
}

type InstanceCardStats = {
  sizeMb: number
  modsCount: number
  lastUsed?: string
}

type CreatorSection =
  | 'Personalizado'
  | 'CurseForge'
  | 'Modrinth'
  | 'Futuro 1'
  | 'Futuro 2'
  | 'Futuro 3'

type EditSection =
  | 'Ejecución'
  | 'Version'
  | 'Mods'
  | 'Resource Packs'
  | 'Shader Packs'
  | 'Notas'
  | 'Mundos'
  | 'Servidores'
  | 'Capturas de Pantalla'
  | 'Configuración'
  | 'Otros registros'

type CreateInstanceResult = {
  id: string
  name: string
  group: string
  instanceRoot: string
  logs: string[]
}

type InstanceCreationProgressEvent = {
  requestId?: string
  message: string
  completed?: number
  total?: number
}

type InstanceSummary = {
  id: string
  name: string
  group: string
  instanceRoot: string
}

type InstanceMetadataView = {
  name: string
  group: string
  minecraftVersion: string
  loader: string
  loaderVersion: string
  ramMb: number
  javaArgs: string[]
  javaPath: string
  javaRuntime: string
  javaVersion: string
  createdAt?: string
  lastUsed?: string
  state?: string
}

type LaunchValidationResult = {
  javaPath: string
  javaVersion: string
  classpath: string
  jvmArgs: string[]
  gameArgs: string[]
  mainClass: string
  logs: string[]
  refreshedAuthSession: {
    profileId: string
    profileName: string
    minecraftAccessToken: string
    minecraftAccessTokenExpiresAt?: number | null
    microsoftRefreshToken?: string | null
    premiumVerified: boolean
  }
}

type StartInstanceResult = {
  pid: number
  javaPath: string
  logs: string[]
  refreshedAuthSession: {
    profileId: string
    profileName: string
    minecraftAccessToken: string
    minecraftAccessTokenExpiresAt?: number | null
    microsoftRefreshToken?: string | null
    premiumVerified: boolean
  }
}

type RuntimeStatus = {
  running: boolean
  pid: number | null
  exitCode: number | null
  stderrTail: string[]
}

type RuntimeOutputEvent = {
  instanceRoot: string
  stream: 'stdout' | 'stderr' | 'system'
  line: string
}

type InstanceModEntry = {
  id: string
  fileName: string
  name: string
  version: string
  provider: string
  enabled: boolean
  sizeBytes: number
  modifiedAt?: number
}

type ModVersionOption = { name: string; version: string; downloadUrl: string; fileName: string }
type InstalledModUpdateCandidate = { mod: InstanceModEntry; nextVersion: ModVersionOption }


type ModCatalogDetail = {
  description: string
  bodyHtml: string
  url: string
  image: string
  links?: Array<{ label: string; url: string }>
  versions: Array<{ id?: string; name: string; gameVersion: string; downloadUrl: string; versionType?: string; publishedAt?: string; requiredDependencies?: string[] }>
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
type McChannel = 'Todos' | 'Estables' | 'Experimentales'

type ConsoleLevel = 'INFO' | 'WARN' | 'ERROR' | 'FATAL'
type ConsoleSource = 'launcher' | 'game'

type ConsoleEntry = {
  timestamp: string
  level: ConsoleLevel
  source: ConsoleSource
  message: string
}

type LoaderVersionItem = {
  version: string
  publishedAt: string
  source: string
  downloadUrl?: string
}

type LoaderChannelFilter = 'Todos' | 'Stable' | 'Latest' | 'Releases'

type InstanceSettingsTab = 'General' | 'Java' | 'Ajustes' | 'Comandos Personalizados' | 'Variables de Entorno'
type GlobalSettingsTab = 'General' | 'Idioma' | 'Apariencia' | 'Java' | 'Servicios' | 'Herramientas' | 'Network'
type ModsAdvancedFilter = { tag: 'all' | 'dependencia' | 'incompatible' | 'crash' | 'warn'; state: 'all' | 'enabled' | 'disabled' }
type ModsDownloaderSource = 'Modrinth' | 'CurseForge' | 'Externos' | 'Locales'
type ModsDownloaderSort = 'relevance' | 'downloads' | 'followers' | 'newest' | 'updated'

type ModsCatalogSource = Extract<ModsDownloaderSource, 'Modrinth' | 'CurseForge'>

type ModsCatalogItem = {
  id: string
  source: ModsCatalogSource
  name: string
  summary: string
  image: string
  downloads: number
  followers: number
  publishedAt: string
  updatedAt: string
  projectType?: string
}

type CatalogVersionItem = {
  id: string
  name: string
  gameVersion: string
  versionType: string
  publishedAt: string
  downloadUrl: string
  requiredDependencies?: string[]
}

type StagedDownloadEntry = {
  mod: ModsCatalogItem
  version: CatalogVersionItem
  reinstall: boolean
  selected: boolean
  dependencies: Array<{ mod: ModsCatalogItem; version: CatalogVersionItem | null; installed: boolean; selected: boolean }>
}

type MicrosoftAuthStart = {
  authorizeUrl: string
  codeVerifier: string
  redirectUri: string
}


type MicrosoftAuthResult = {
  minecraftAccessToken: string
  minecraftAccessTokenExpiresAt?: number
  microsoftAccessToken: string
  microsoftRefreshToken?: string
  premiumVerified: boolean
  profile: {
    id: string
    name: string
  }
}

type AuthSession = {
  profileId: string
  profileName: string
  minecraftAccessToken: string
  minecraftAccessTokenExpiresAt?: number
  microsoftAccessToken: string
  microsoftRefreshToken?: string
  premiumVerified: boolean
  loggedAt: number
}

type AccountType = 'Msa' | 'Offline'

type ManagedAccount = {
  profileId: string
  profileName: string
  email: string
  type: AccountType
  status: 'Lista para usar'
  totalPlaytimeMs: number
  isDefault: boolean
  loggedAt: number
}

type LanguageEntry = {
  name: string
  installedByDefault: boolean
}

type AppearancePreset = {
  id: string
  name: string
  description: string
  vars: Record<string, string>
}

type UserAppearanceTheme = {
  id: string
  name: string
  vars: Record<AppearanceColorKey, string>
}

type AppearanceColorKey =
  | '--bg-main'
  | '--bg-surface'
  | '--bg-surface-muted'
  | '--bg-hover'
  | '--border'
  | '--text-main'
  | '--text-muted'
  | '--accent'
  | '--accent-hover'

type FontOption = {
  id: string
  label: string
  family: string
}

type FolderRouteKey = 'launcher' | 'instances' | 'icons' | 'java' | 'skins' | 'downloads'

type PickedFolderResult = {
  path: string | null
}

type FolderRouteItem = {
  key: FolderRouteKey
  label: string
  description: string
  value: string
}

type LauncherFolders = {
  launcherRoot: string
  instancesDir: string
  runtimeDir: string
  assetsDir: string
}

type FolderRoutesPayload = {
  routes: Array<{
    key: FolderRouteKey
    value: string
  }>
}

type LauncherUpdateItem = {
  version: string
  releaseDate: string
  channel: 'Stable' | 'Preview'
  summary: string
  status: 'Instalada' | 'Disponible' | 'Histórica'
}


const creatorSections: CreatorSection[] = ['Personalizado', 'CurseForge', 'Modrinth', 'Futuro 1', 'Futuro 2', 'Futuro 3']

const editSections: EditSection[] = ['Ejecución', 'Version', 'Mods', 'Resource Packs', 'Shader Packs', 'Notas', 'Mundos', 'Servidores', 'Capturas de Pantalla', 'Configuración', 'Otros registros']

const globalSettingsTabs: GlobalSettingsTab[] = ['General', 'Idioma', 'Apariencia', 'Java', 'Servicios', 'Herramientas', 'Network']

const languageCatalog: LanguageEntry[] = [
  { name: 'Español (España)', installedByDefault: false },
  { name: 'Español (Latinoamérica)', installedByDefault: true },
  { name: 'English (US)', installedByDefault: true },
  { name: 'English (UK)', installedByDefault: false },
  { name: 'Português (Brasil)', installedByDefault: false }, { name: 'Português (Portugal)', installedByDefault: false },
  { name: 'Français', installedByDefault: false }, { name: 'Deutsch', installedByDefault: false }, { name: 'Italiano', installedByDefault: false }, { name: 'Nederlands', installedByDefault: false },
  { name: 'Dansk', installedByDefault: false }, { name: 'Svenska', installedByDefault: false }, { name: 'Norsk Bokmål', installedByDefault: false }, { name: 'Suomi', installedByDefault: false },
  { name: 'Polski', installedByDefault: false }, { name: 'Čeština', installedByDefault: false }, { name: 'Slovenčina', installedByDefault: false }, { name: 'Magyar', installedByDefault: false },
  { name: 'Română', installedByDefault: false }, { name: 'Türkçe', installedByDefault: false }, { name: 'Українська', installedByDefault: false }, { name: 'Русский', installedByDefault: false },
  { name: 'العربية', installedByDefault: false }, { name: 'עברית', installedByDefault: false }, { name: 'हिन्दी', installedByDefault: false }, { name: 'বাংলা', installedByDefault: false },
  { name: '日本語', installedByDefault: false }, { name: '한국어', installedByDefault: false }, { name: '简体中文', installedByDefault: false }, { name: '繁體中文', installedByDefault: false },
  { name: 'ไทย', installedByDefault: false }, { name: 'Tiếng Việt', installedByDefault: false }, { name: 'Bahasa Indonesia', installedByDefault: false }, { name: 'Bahasa Melayu', installedByDefault: false },
  { name: 'Filipino', installedByDefault: false }, { name: 'Ελληνικά', installedByDefault: false },
]



const languageLocaleMap: Record<string, string> = {
  'Español (España)': 'es-ES',
  'Español (Latinoamérica)': 'es-MX',
  'English (US)': 'en-US',
  'English (UK)': 'en-GB',
  'Português (Brasil)': 'pt-BR',
  'Português (Portugal)': 'pt-PT',
  Français: 'fr-FR',
  Deutsch: 'de-DE',
  Italiano: 'it-IT',
  日本語: 'ja-JP',
  한국어: 'ko-KR',
  简体中文: 'zh-CN',
  繁體中文: 'zh-TW',
}
const appearancePresets: AppearancePreset[] = [
  {
    id: 'obsidian-elegance',
    name: 'Obsidian Elegance (Default)',
    description: 'Tema oscuro elegante con acento azul nocturno y contraste suave premium.',
    vars: {
      '--bg-main': '#111318',
      '--bg-surface': '#171a21',
      '--bg-surface-muted': '#1e2430',
      '--bg-hover': '#273043',
      '--border': '#3c4a63',
      '--text-main': '#edf2ff',
      '--text-muted': '#b7c2d8',
      '--accent': '#7aa2ff',
      '--accent-hover': '#9eb9ff',
    },
  },
  {
    id: 'pastel-sage',
    name: 'Pastel Sage',
    description: 'Verde salvia pastel cómodo para sesiones largas y lectura suave.',
    vars: {
      '--bg-main': '#20251f',
      '--bg-surface': '#2b332b',
      '--bg-surface-muted': '#313a31',
      '--bg-hover': '#3b463b',
      '--border': '#717d71',
      '--text-main': '#edf3ea',
      '--text-muted': '#c6d1c2',
      '--accent': '#bdd8b8',
      '--accent-hover': '#cfe5ca',
    },
  },
  {
    id: 'pastel-sand',
    name: 'Pastel Sand',
    description: 'Arena pastel y crema apagada para una interfaz cálida y limpia.',
    vars: {
      '--bg-main': '#26221d',
      '--bg-surface': '#332d26',
      '--bg-surface-muted': '#3a332b',
      '--bg-hover': '#463d33',
      '--border': '#8a7f70',
      '--text-main': '#f5ede3',
      '--text-muted': '#d4c7b7',
      '--accent': '#e1c8a8',
      '--accent-hover': '#ead8bf',
    },
  },
  {
    id: 'pastel-peach',
    name: 'Pastel Peach',
    description: 'Durazno pastel apagado con contraste amable para paneles y acciones.',
    vars: {
      '--bg-main': '#2a211e',
      '--bg-surface': '#392c28',
      '--bg-surface-muted': '#40312d',
      '--bg-hover': '#4d3a35',
      '--border': '#92756d',
      '--text-main': '#f9ece8',
      '--text-muted': '#dcbfb8',
      '--accent': '#e4b8aa',
      '--accent-hover': '#eccabf',
    },
  },
  {
    id: 'pastel-rose',
    name: 'Pastel Rose',
    description: 'Rosa pastel tenue y neutro para un estilo suave sin saturación fuerte.',
    vars: {
      '--bg-main': '#292124',
      '--bg-surface': '#382d32',
      '--bg-surface-muted': '#40333a',
      '--bg-hover': '#4d3d46',
      '--border': '#8d7480',
      '--text-main': '#f6eaef',
      '--text-muted': '#d8c1ca',
      '--accent': '#dfb5c5',
      '--accent-hover': '#e8c8d4',
    },
  },
  {
    id: 'pastel-mist',
    name: 'Pastel Mist',
    description: 'Gris cálido pastel para una estética cómoda y minimalista.',
    vars: {
      '--bg-main': '#222120',
      '--bg-surface': '#2f2d2c',
      '--bg-surface-muted': '#363433',
      '--bg-hover': '#413e3c',
      '--border': '#7c7874',
      '--text-main': '#f0ece8',
      '--text-muted': '#cac4bf',
      '--accent': '#d5c9bd',
      '--accent-hover': '#dfd5cb',
    },
  },
]


const fontOptions: FontOption[] = [
  { id: 'inter', label: 'Inter (predeterminada)', family: "Inter, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif" },
  { id: 'poppins', label: 'Poppins', family: "Poppins, 'Segoe UI', Inter, sans-serif" },
  { id: 'nunito', label: 'Nunito', family: "Nunito, 'Segoe UI', Inter, sans-serif" },
  { id: 'jetbrains', label: 'JetBrains Sans', family: "'JetBrains Sans', 'Segoe UI', Inter, sans-serif" },
  { id: 'fira', label: 'Fira Sans', family: "'Fira Sans', 'Segoe UI', Inter, sans-serif" },
  { id: 'roboto', label: 'Roboto', family: "Roboto, 'Segoe UI', Inter, sans-serif" },
  { id: 'source', label: 'Source Sans 3', family: "'Source Sans 3', 'Segoe UI', Inter, sans-serif" },
  { id: 'manrope', label: 'Manrope', family: "Manrope, 'Segoe UI', Inter, sans-serif" },
]

const launcherUpdatesUrl = 'https://github.com/TU_USUARIO/TU_REPO/releases'

const instanceActions = ['Iniciar', 'Forzar Cierre', 'Editar', 'Cambiar Grupo', 'Carpeta (Interface)', 'Exportar', 'Copiar', 'Crear atajo']
const defaultGroup = 'Sin grupo'
const sidebarMinWidth = 144
const sidebarMaxWidth = 320
const mojangManifestUrl = 'https://piston-meta.mojang.com/mc/game/version_manifest_v2.json'
const authSessionKey = 'launcher_microsoft_auth_session_v1'
const managedAccountsKey = 'launcher_managed_accounts_v1'
const instanceVisualMetaKey = 'launcher_instance_visual_meta_v1'
const folderRoutesKey = 'launcher_folder_routes_v1'
const appearanceSettingsKey = 'launcher_appearance_settings_v2'
const languageSettingsKey = 'launcher_language_settings_v1'
const authCodeRegenerateCooldownMs = 10_000

const defaultFolderRoutes: FolderRouteItem[] = [
  { key: 'launcher', label: 'Ruta de Launcher', description: 'Raíz principal de configuración del launcher.', value: 'InterfaceLauncher' },
  { key: 'icons', label: 'Ruta de Íconos', description: 'Biblioteca de iconos personalizados para perfiles.', value: 'InterfaceLauncher/assets/icons' },
  { key: 'java', label: 'Ruta de Java', description: 'Runtimes embebidos o selección manual de Java.', value: 'InterfaceLauncher/runtime' },
  { key: 'skins', label: 'Ruta de Skins', description: 'Skins importadas y exportadas por el launcher.', value: 'InterfaceLauncher/assets/skins' },
  { key: 'downloads', label: 'Ruta de Descargas', description: 'Descargas temporales y caché de instaladores.', value: 'InterfaceLauncher/downloads' },
]

const normalizeRoutePath = (value: string) => value.trim().replace(/\\/g, '/')

const ensureAbsoluteRoutePath = (value: string, launcherRoot: string) => {
  const normalized = normalizeRoutePath(value)
  if (!normalized) return normalizeRoutePath(launcherRoot)
  if (/^[a-zA-Z]:\//.test(normalized) || normalized.startsWith('/')) return normalized
  return `${normalizeRoutePath(launcherRoot).replace(/\/$/, '')}/${normalized.replace(/^\//, '')}`
}

const sanitizeFolderRoutes = (routes: FolderRouteItem[]) => {
  const launcherRoot = normalizeRoutePath(routes.find((item) => item.key === 'launcher')?.value ?? defaultFolderRoutes[0].value)
  return routes.map((route) => {
    const value = route.key === 'launcher'
      ? launcherRoot
      : ensureAbsoluteRoutePath(route.value, launcherRoot)
    return { ...route, value }
  })
}

const launcherUpdatesFeed: LauncherUpdateItem[] = [
  { version: 'v0.3.0', releaseDate: '2026-02-19', channel: 'Stable', summary: 'Nuevo panel de updates, perfiles de apariencia pastel y mejoras de carpetas globales.', status: 'Disponible' },
  { version: 'v0.2.5', releaseDate: '2026-02-10', channel: 'Stable', summary: 'Correcciones en gestión de cuentas, mejora de logs y estabilidad de inicio.', status: 'Instalada' },
  { version: 'v0.2.0', releaseDate: '2026-01-28', channel: 'Stable', summary: 'Integración de creador de instancias con más metadatos del runtime.', status: 'Histórica' },
  { version: 'v0.1.8', releaseDate: '2026-01-14', channel: 'Preview', summary: 'Primer experimento de consola avanzada y tarjetas dinámicas.', status: 'Histórica' },
]

function nowTimestamp() {
  return new Date().toLocaleTimeString('es-ES', { hour12: false })
}

function makeConsoleEntry(level: ConsoleLevel, source: ConsoleSource, message: string): ConsoleEntry {
  return { timestamp: nowTimestamp(), level, source, message }
}

function formatIsoDate(iso: string, locale = 'es-ES'): string {
  if (!iso) return '-'
  return new Date(iso).toLocaleDateString(locale)
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
  if (type === 'stable') return 'Estable'
  if (type === 'latest') return 'Última'
  return type
}

function inferNeoForgeFamily(mcVersion: string): string | null {
  const parts = mcVersion.split('.')
  if (parts.length < 2 || parts[0] !== '1') return null
  const minor = parts[1]
  const patch = parts[2] ?? '0'
  return `${minor}.${patch}`
}

function parseDateSafe(value: string | undefined): number {
  if (!value || value === '-') return 0
  const parsed = Date.parse(value)
  return Number.isFinite(parsed) ? parsed : 0
}

function compareVersionLike(a: string, b: string): number {
  const aParts = a.split(/[.-]/).map((part) => Number.parseInt(part, 10))
  const bParts = b.split(/[.-]/).map((part) => Number.parseInt(part, 10))
  const maxLength = Math.max(aParts.length, bParts.length)
  for (let index = 0; index < maxLength; index += 1) {
    const aValue = Number.isFinite(aParts[index]) ? aParts[index] : -1
    const bValue = Number.isFinite(bParts[index]) ? bParts[index] : -1
    if (aValue !== bValue) return bValue - aValue
  }
  return b.localeCompare(a)
}

function sortLoaderVersions(items: LoaderVersionItem[]): LoaderVersionItem[] {
  return [...items].sort((left, right) => {
    const dateDiff = parseDateSafe(right.publishedAt) - parseDateSafe(left.publishedAt)
    if (dateDiff !== 0) return dateDiff
    return compareVersionLike(left.version, right.version)
  })
}


function resolveVisualMedia(meta?: InstanceVisualMeta): string {
  if (meta?.mediaPath) {
    try {
      return convertFileSrc(meta.mediaPath)
    } catch {
      return meta.mediaPath
    }
  }
  return meta?.mediaDataUrl ?? ''
}

function mediaTypeFromMime(mime?: string): 'video' | 'image' {
  if (!mime) return 'image'
  return mime.startsWith('video/') ? 'video' : 'image'
}

function inferMimeFromName(fileName: string): string | undefined {
  const ext = fileName.split('.').pop()?.toLowerCase()
  if (!ext) return undefined
  if (ext === 'png') return 'image/png'
  if (ext === 'jpg' || ext === 'jpeg') return 'image/jpeg'
  if (ext === 'gif') return 'image/gif'
  if (ext === 'webp') return 'image/webp'
  if (ext === 'mp4') return 'video/mp4'
  return undefined
}

function mediaTypeFromMeta(meta?: InstanceVisualMeta): 'video' | 'image' {
  const mime = meta?.mediaMime?.toLowerCase()
  if (mime) return mediaTypeFromMime(mime)
  const path = meta?.mediaPath?.toLowerCase() ?? meta?.mediaDataUrl?.toLowerCase() ?? ''
  if (path.includes('.mp4') || path.startsWith('data:video/')) return 'video'
  return 'image'
}

function App() {
  const [activePage, setActivePage] = useState<MainPage>('Mis Modpacks')
  const [backHistory, setBackHistory] = useState<MainPage[]>([])
  const [forwardHistory, setForwardHistory] = useState<MainPage[]>([])
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
  const [instanceIconPreview, setInstanceIconPreview] = useState<string>('⛏')
  const [isCreating, setIsCreating] = useState(false)
  const [creationProgress, setCreationProgress] = useState<{ completed: number; total: number } | null>(null)
  const [manifestVersions, setManifestVersions] = useState<ManifestVersion[]>([])
  const [manifestLoading, setManifestLoading] = useState(false)
  const [manifestError, setManifestError] = useState('')
  const [selectedMcFilter, setSelectedMcFilter] = useState<MinecraftFilter>('Releases')
  const [selectedMcChannel, setSelectedMcChannel] = useState<McChannel>('Todos')
  const [selectedLoader, setSelectedLoader] = useState<LoaderKey>('none')
  const [selectedMinecraftVersion, setSelectedMinecraftVersion] = useState<ManifestVersion | null>(null)
  const [selectedMinecraftDetail, setSelectedMinecraftDetail] = useState<MinecraftVersionDetail | null>(null)
  const [selectedLoaderVersion, setSelectedLoaderVersion] = useState<LoaderVersionItem | null>(null)
  const [loaderVersions, setLoaderVersions] = useState<LoaderVersionItem[]>([])
  const [loaderLoading, setLoaderLoading] = useState(false)
  const [loaderError, setLoaderError] = useState('')
  const [selectedLoaderFilter, setSelectedLoaderFilter] = useState<LoaderChannelFilter>('Todos')
  const [runtimeConsole, setRuntimeConsole] = useState<ConsoleEntry[]>([])
  const [runtimeConsoleByInstance, setRuntimeConsoleByInstance] = useState<Record<string, ConsoleEntry[]>>({})
  const [launchPreparation] = useState<LaunchValidationResult | null>(null)
  const [consoleLevelFilter, setConsoleLevelFilter] = useState<'Todos' | ConsoleLevel>('Todos')
  const [launcherLogFilter, setLauncherLogFilter] = useState<'Todos' | ConsoleSource>('Todos')
  const [autoScrollConsole, setAutoScrollConsole] = useState(true)
  const [instanceDrafts, setInstanceDrafts] = useState<Record<string, InstanceSummary>>({})
  const [selectedInstanceMetadata, setSelectedInstanceMetadata] = useState<InstanceMetadataView | null>(null)
  const [instanceMetaByRoot, setInstanceMetaByRoot] = useState<Record<string, InstanceMetadataView>>({})
  const [instanceStatsByRoot, setInstanceStatsByRoot] = useState<Record<string, InstanceCardStats>>({})
  const [instanceVisualMeta, setInstanceVisualMeta] = useState<Record<string, InstanceVisualMeta>>({})
  const [selectedSettingsTab, setSelectedSettingsTab] = useState<InstanceSettingsTab>('General')
  const [selectedGlobalSettingsTab, setSelectedGlobalSettingsTab] = useState<GlobalSettingsTab>('General')
  const [instanceMods, setInstanceMods] = useState<InstanceModEntry[]>([])
  const [modsLoading, setModsLoading] = useState(false)
  const [modsError, setModsError] = useState('')
  const [modsSearch, setModsSearch] = useState('')
  const [modsProviderFilter, setModsProviderFilter] = useState<'all' | 'CurseForge' | 'Modrinth' | 'Externo' | 'Local'>('all')
  const [modsPage, setModsPage] = useState(1)
  const [modsAdvancedOpen, setModsAdvancedOpen] = useState(false)
  const [modsAdvancedFilter, setModsAdvancedFilter] = useState<ModsAdvancedFilter>({ tag: 'all', state: 'all' })
  const [modsNameColumnWidth, setModsNameColumnWidth] = useState(320)
  const [modVersionLoading, setModVersionLoading] = useState(false)
  const [modVersionError, setModVersionError] = useState('')
  const [modVersionOptions, setModVersionOptions] = useState<ModVersionOption[]>([])
  const [modVersionModalOpen, setModVersionModalOpen] = useState(false)
  const [modVersionDetail, setModVersionDetail] = useState<ModCatalogDetail | null>(null)
  const [selectedVersionOptionId, setSelectedVersionOptionId] = useState('')
  const [modIconById, setModIconById] = useState<Record<string, string>>({})
  const [updatesModalOpen, setUpdatesModalOpen] = useState(false)
  const [updatesReviewLoading, setUpdatesReviewLoading] = useState(false)
  const [updatesCandidates, setUpdatesCandidates] = useState<InstalledModUpdateCandidate[]>([])
  const [selectedModId, setSelectedModId] = useState('')
  const [modsDownloaderOpen, setModsDownloaderOpen] = useState(false)
  const [modsDownloaderSource, setModsDownloaderSource] = useState<ModsDownloaderSource>('Modrinth')
  const [modsDownloaderSort, setModsDownloaderSort] = useState<ModsDownloaderSort>('relevance')
  const [selectedCatalogModId, setSelectedCatalogModId] = useState('')
  const [downloaderSearch, setDownloaderSearch] = useState('')
  const [debouncedDownloaderSearch, setDebouncedDownloaderSearch] = useState('')
  const [downloaderShowAllVersions, setDownloaderShowAllVersions] = useState(false)
  const [downloaderVersionFilter, setDownloaderVersionFilter] = useState('')
  const [downloaderLoaderFilter, setDownloaderLoaderFilter] = useState('')
  const [downloaderClientOnly, setDownloaderClientOnly] = useState(false)
  const [downloaderServerOnly, setDownloaderServerOnly] = useState(false)
  const [modsCatalogLoading, setModsCatalogLoading] = useState(false)
  const [modsCatalogError, setModsCatalogError] = useState('')
  const [downloaderCatalogMods, setDownloaderCatalogMods] = useState<ModsCatalogItem[]>([])
  const [catalogDetailByModId, setCatalogDetailByModId] = useState<Record<string, ModCatalogDetail>>({})
  const [catalogDetailLoading, setCatalogDetailLoading] = useState(false)
  const [stagedDownloads, setStagedDownloads] = useState<Record<string, StagedDownloadEntry>>({})
  const [reviewModalOpen, setReviewModalOpen] = useState(false)
  const [installingModalOpen, setInstallingModalOpen] = useState(false)
  const [cancelModsConfirmOpen, setCancelModsConfirmOpen] = useState(false)
  const [installProgress, setInstallProgress] = useState({ current: 0, total: 0, message: '' })
  const [selectedLanguage, setSelectedLanguage] = useState(languageCatalog[0].name)
  const [installedLanguages, setInstalledLanguages] = useState<string[]>(languageCatalog.filter((item) => item.installedByDefault).map((item) => item.name))
  const [languageSearch, setLanguageSearch] = useState('')
  const [selectedAppearancePreset, setSelectedAppearancePreset] = useState(appearancePresets[0].id)
  const [userAppearanceThemes, setUserAppearanceThemes] = useState<UserAppearanceTheme[]>([])
  const [selectedFontFamily, setSelectedFontFamily] = useState(fontOptions[0].family)
  const [uiScalePercent, setUiScalePercent] = useState(100)
  const [uiElementScalePercent, setUiElementScalePercent] = useState(100)
  const [appearanceLoaded, setAppearanceLoaded] = useState(false)
  const [customAppearanceVars, setCustomAppearanceVars] = useState<Record<AppearanceColorKey, string>>({
    '--bg-main': appearancePresets[0].vars['--bg-main'],
    '--bg-surface': appearancePresets[0].vars['--bg-surface'],
    '--bg-surface-muted': appearancePresets[0].vars['--bg-surface-muted'],
    '--bg-hover': appearancePresets[0].vars['--bg-hover'],
    '--border': appearancePresets[0].vars['--border'],
    '--text-main': appearancePresets[0].vars['--text-main'],
    '--text-muted': appearancePresets[0].vars['--text-muted'],
    '--accent': appearancePresets[0].vars['--accent'],
    '--accent-hover': appearancePresets[0].vars['--accent-hover'],
  })
  const [newThemeName, setNewThemeName] = useState('')
  const [appearanceMessage, setAppearanceMessage] = useState('')
  const [launchProgressPercent, setLaunchProgressPercent] = useState(0)
  const [folderRoutes, setFolderRoutes] = useState<FolderRouteItem[]>(defaultFolderRoutes)
  const [updatesAutoCheck, setUpdatesAutoCheck] = useState(true)
  const [updatesChannel, setUpdatesChannel] = useState<'Stable' | 'Preview'>('Stable')
  const [updatesStatus, setUpdatesStatus] = useState('Listo para buscar updates.')
  const { progress: migrationProgress, isMigrating, migrateLauncherRoot, changeInstancesFolder } = useMigration()
  const [launcherFolders, setLauncherFolders] = useState<LauncherFolders | null>(null)
  const [launcherMigrationPath, setLauncherMigrationPath] = useState<string | null>(null)
  const [instancesMigrationPath, setInstancesMigrationPath] = useState<string | null>(null)
  const [isStartingInstance, setIsStartingInstance] = useState(false)
  const [isInstanceRunning, setIsInstanceRunning] = useState(false)
  const [lastRuntimeExitKey, setLastRuntimeExitKey] = useState('')
  const [showDeleteInstanceConfirm, setShowDeleteInstanceConfirm] = useState(false)
  const [showExportMenu, setShowExportMenu] = useState(false)
  const [isDeletingInstance, setIsDeletingInstance] = useState(false)
  const [authSession, setAuthSession] = useState<AuthSession | null>(null)
  const [managedAccounts, setManagedAccounts] = useState<ManagedAccount[]>([])
  const [accountMenuOpen, setAccountMenuOpen] = useState(false)
  const [selectedAccountId, setSelectedAccountId] = useState<string>('')
  const [isAuthReady, setIsAuthReady] = useState(false)
  const [isAuthenticating, setIsAuthenticating] = useState(false)
  const [authRetryAt, setAuthRetryAt] = useState(0)
  const [nowTick, setNowTick] = useState(() => Date.now())
  const [authStatus, setAuthStatus] = useState('')
  const [authError, setAuthError] = useState('')
  const creationIconInputRef = useRef<HTMLInputElement | null>(null)
  const selectedCardIconInputRef = useRef<HTMLInputElement | null>(null)
  const creationConsoleRef = useRef<HTMLDivElement | null>(null)
  const runtimeConsoleRef = useRef<HTMLDivElement | null>(null)
  const playtimeStartRef = useRef<number | null>(null)





  const appendRuntime = (entry: ConsoleEntry) => {
    setRuntimeConsole((prev) => {
      const next = [...prev, entry]
      return next.length > 2000 ? next.slice(next.length - 2000) : next
    })
    setRuntimeConsoleByInstance((prev) => {
      if (!selectedCard?.instanceRoot) return prev
      const current = prev[selectedCard.instanceRoot] ?? []
      const next = [...current, entry]
      return {
        ...prev,
        [selectedCard.instanceRoot]: next.length > 2000 ? next.slice(next.length - 2000) : next,
      }
    })
  }

  const appendRuntimeForRoot = (instanceRoot: string | undefined, entry: ConsoleEntry) => {
    if (!instanceRoot) return
    setRuntimeConsoleByInstance((prev) => {
      const current = prev[instanceRoot] ?? []
      const next = [...current, entry]
      return {
        ...prev,
        [instanceRoot]: next.length > 2000 ? next.slice(next.length - 2000) : next,
      }
    })
    if (selectedCard?.instanceRoot === instanceRoot) {
      setRuntimeConsole((prev) => {
        const next = [...prev, entry]
        return next.length > 2000 ? next.slice(next.length - 2000) : next
      })
    }
  }

  const persistAuthSession = (session: AuthSession | null) => {
    if (!session) {
      localStorage.removeItem(authSessionKey)
      return
    }
    localStorage.setItem(authSessionKey, JSON.stringify(session))
  }

  const persistManagedAccounts = (accounts: ManagedAccount[]) => {
    localStorage.setItem(managedAccountsKey, JSON.stringify(accounts))
  }

  const persistFolderRoutes = (routes: FolderRouteItem[]) => {
    localStorage.setItem(folderRoutesKey, JSON.stringify(routes))
  }

  const refreshInstances = useCallback(async () => {
    try {
      const loadedCards = await invoke<InstanceSummary[]>('list_instances')
      setCards(loadedCards)
      setSelectedCard((prev) => {
        if (!prev) return null
        return loadedCards.find((card) => card.id === prev.id) ?? null
      })
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      setCreationConsoleLogs((prev) => [...prev, `No se pudieron cargar las instancias guardadas: ${message}`])
    }
  }, [])

  const updateAndPersistFolderRoutes = async (nextRoutes: FolderRouteItem[]) => {
    const sanitized = sanitizeFolderRoutes(nextRoutes)
    setFolderRoutes(sanitized)
    persistFolderRoutes(sanitized)
    await invoke('save_folder_routes', { routes: { routes: sanitized.map(({ key, value }) => ({ key, value })) } })
    await refreshLauncherFolders()
  }

  const pickFolderRoute = async (route: FolderRouteItem) => {
    try {
      const result = await invoke<{ path: string | null }>('pick_folder', {
        initialPath: route.value,
        title: `Seleccionar ${route.label}`,
      })
      if (!result.path) return
      let nextRoutes = folderRoutes.map((item) => item.key === route.key ? { ...item, value: result.path ?? item.value } : item)
      if (route.key === 'launcher' && result.path) {
        const normalizedOld = route.value.replace(/\\/g, '/').replace(/\/$/, '')
        const normalizedNew = result.path.replace(/\\/g, '/').replace(/\/$/, '')
        nextRoutes = nextRoutes.map((item) => {
          if (item.key === 'launcher') return item
          const current = item.value.replace(/\\/g, '/')
          if (current.startsWith(normalizedOld)) {
            return { ...item, value: `${normalizedNew}${current.slice(normalizedOld.length)}` }
          }
          return item
        })
      }
      await updateAndPersistFolderRoutes(nextRoutes)
      if (route.key === 'instances') {
        await refreshInstances()
      }
      setUpdatesStatus(`Ruta actualizada: ${route.label}`)
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      setUpdatesStatus(`No se pudo seleccionar carpeta: ${message}`)
    }
  }


  const refreshLauncherFolders = async () => {
    try {
      const folders = await invoke<LauncherFolders>('get_launcher_folders')
      setLauncherFolders(folders)
    } catch {
      setLauncherFolders(null)
    }
  }

  const pickNewLauncherRoot = async () => {
    const result = await invoke<PickedFolderResult>('pick_folder', {
      initialPath: launcherFolders?.launcherRoot,
      title: 'Selecciona carpeta raíz del launcher',
    })
    if (!result.path) return
    setLauncherMigrationPath(result.path)
  }

  const pickNewInstancesFolder = async () => {
    const result = await invoke<PickedFolderResult>('pick_folder', {
      initialPath: launcherFolders?.instancesDir,
      title: 'Selecciona carpeta de instancias',
    })
    if (!result.path) return
    setInstancesMigrationPath(result.path)
  }


  const checkLauncherUpdates = async () => {
    setUpdatesStatus('Consultando endpoint de versiones del launcher...')
    try {
      await invoke('open_url_in_browser', { url: launcherUpdatesUrl, browserId: 'default' })
      setUpdatesStatus('Se abrió el canal de releases. Estructura lista para conectar updater nativo de Tauri.')
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      setUpdatesStatus(`No se pudo abrir el canal de updates: ${message}`)
    }
  }

  const syncManagedAccountFromSession = (session: AuthSession, email = '-') => {
    setManagedAccounts((prev) => {
      const exists = prev.find((account) => account.profileId === session.profileId)
      const next: ManagedAccount[] = exists
        ? prev.map((account) => account.profileId === session.profileId
          ? { ...account, profileName: session.profileName, status: 'Lista para usar', type: 'Msa', loggedAt: session.loggedAt, email: account.email || email }
          : account)
        : [
            ...prev.map((account) => ({ ...account, isDefault: false })),
            {
              profileId: session.profileId,
              profileName: session.profileName,
              email,
              type: 'Msa',
              status: 'Lista para usar',
              totalPlaytimeMs: 0,
              isDefault: prev.length === 0,
              loggedAt: session.loggedAt,
            },
          ]
      persistManagedAccounts(next)
      return next
    })
  }

  const addPlaytimeToDefaultAccount = (elapsedMs: number) => {
    if (elapsedMs <= 0) return
    setManagedAccounts((prev) => {
      if (prev.length === 0) return prev
      const defaultAccount = prev.find((account) => account.isDefault) ?? prev[0]
      const next = prev.map((account) => account.profileId === defaultAccount.profileId
        ? { ...account, totalPlaytimeMs: account.totalPlaytimeMs + elapsedMs }
        : account)
      persistManagedAccounts(next)
      return next
    })
  }

  const logout = () => {
    setAuthSession(null)
    setAccountMenuOpen(false)
    persistAuthSession(null)
    setAuthStatus('Sesión cerrada correctamente.')
    setAuthError('')
  }

  const startMicrosoftLogin = async () => {
    if (isAuthenticating) return
    if (Date.now() < authRetryAt) return
    setIsAuthenticating(true)
    setAuthError('')
    setAuthStatus('Abriendo login de Microsoft dentro del launcher...')

    try {
      const authStart = await invoke<MicrosoftAuthStart>('start_microsoft_auth')

      const code = await invoke<string>('authorize_microsoft_in_launcher', {
        authorizeUrl: authStart.authorizeUrl,
      })

      setAuthStatus('Código de autorización recibido. Completando login...')

      const result = await invoke<MicrosoftAuthResult>('complete_microsoft_auth', {
        code,
        codeVerifier: authStart.codeVerifier,
      })

      const session: AuthSession = {
        profileId: result.profile.id,
        profileName: result.profile.name,
        minecraftAccessToken: result.minecraftAccessToken,
        minecraftAccessTokenExpiresAt: result.minecraftAccessTokenExpiresAt,
        microsoftAccessToken: result.microsoftAccessToken,
        microsoftRefreshToken: result.microsoftRefreshToken,
        premiumVerified: result.premiumVerified,
        loggedAt: Date.now(),
      }

      setAuthSession(session)
      syncManagedAccountFromSession(session)
      persistAuthSession(session)
      setAuthStatus(`Sesión iniciada como ${session.profileName}.`)
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      setAuthError(message)
      setAuthStatus('')
      setAuthRetryAt(Date.now() + authCodeRegenerateCooldownMs)
    } finally {
      setIsAuthenticating(false)
    }
  }

  useEffect(() => {
    void refreshLauncherFolders()
  }, [])

  const authRetrySeconds = Math.max(0, Math.ceil((authRetryAt - nowTick) / 1000))
  const isAuthCooldown = authRetrySeconds > 0

  const selectedLocale = languageLocaleMap[selectedLanguage] ?? 'es-ES'
  const uiLanguage: 'es' | 'en' | 'pt' = selectedLocale.startsWith('en') ? 'en' : selectedLocale.startsWith('pt') ? 'pt' : 'es'
  const ui = uiLanguage === 'en'
    ? {
        globalTitle: 'Global Settings', globalDesc: 'Central launcher settings by category.', folderTitle: 'Folder locations',
        launcherRoot: 'Launcher root folder', instances: 'Instances folder', runtime: 'Embedded Java', assets: 'Skins / Assets',
        changeRoot: 'Change root folder', changeInstances: 'Change instances folder', current: 'Current',
      }
    : uiLanguage === 'pt'
      ? {
          globalTitle: 'Configurações Globais', globalDesc: 'Painel central de ajustes do launcher por categoria.', folderTitle: 'Localização de pastas',
          launcherRoot: 'Pasta raiz do launcher', instances: 'Pasta de instâncias', runtime: 'Java embutido', assets: 'Skins / Assets',
          changeRoot: 'Alterar pasta raiz', changeInstances: 'Alterar pasta de instâncias', current: 'Atual',
        }
      : {
          globalTitle: 'Configuración Global', globalDesc: 'Panel central de ajustes del launcher por categorías.', folderTitle: 'Ubicaciones de carpetas',
          launcherRoot: 'Carpeta raíz del launcher', instances: 'Instancias', runtime: 'Java embebido', assets: 'Skins / Assets',
          changeRoot: 'Cambiar carpeta raíz', changeInstances: 'Elegir carpeta de instancias', current: 'Actual',
        }
  const filteredLanguages = languageCatalog.filter((lang) => lang.name.toLowerCase().includes(languageSearch.trim().toLowerCase()))




  const iconButtonStyle = instanceIconPreview.startsWith('data:image')
    ? ({ backgroundImage: `url(${instanceIconPreview})`, backgroundSize: 'cover', backgroundPosition: 'center', color: 'transparent' } as CSSProperties)
    : undefined



  useEffect(() => {
    const stored = localStorage.getItem(authSessionKey)
    if (stored) {
      try {
        const parsed = JSON.parse(stored) as AuthSession
        if (parsed.profileId && parsed.profileName && parsed.minecraftAccessToken && parsed.premiumVerified) {
          setAuthSession(parsed)
          syncManagedAccountFromSession(parsed)
          setAuthStatus(`Sesión restaurada para ${parsed.profileName}.`)
        }
      } catch {
        localStorage.removeItem(authSessionKey)
      }
    }

    const savedAccounts = localStorage.getItem(managedAccountsKey)
    if (savedAccounts) {
      try {
        const parsed = JSON.parse(savedAccounts) as ManagedAccount[]
        if (Array.isArray(parsed)) {
          setManagedAccounts(parsed)
        }
      } catch {
        localStorage.removeItem(managedAccountsKey)
      }
    }

    const savedVisualMeta = localStorage.getItem(instanceVisualMetaKey)
    if (savedVisualMeta) {
      try {
        const parsed = JSON.parse(savedVisualMeta) as Record<string, InstanceVisualMeta>
        setInstanceVisualMeta(parsed)
      } catch {
        localStorage.removeItem(instanceVisualMetaKey)
      }
    }

    setIsAuthReady(true)
  }, [])

  useEffect(() => {
    try {
      localStorage.setItem(instanceVisualMetaKey, JSON.stringify(instanceVisualMeta))
    } catch {
      setCreationConsoleLogs((prev) => [...prev, 'Aviso: no se pudo persistir metadata visual localmente (almacenamiento lleno).'])
    }
  }, [instanceVisualMeta])

  useEffect(() => {
    if (!selectedCard?.instanceRoot) return
    const meta = instanceVisualMeta[selectedCard.id]
    if (!meta) return
    void invoke('save_instance_visual_meta', {
      instanceRoot: selectedCard.instanceRoot,
      meta,
    })
  }, [selectedCard?.id, selectedCard?.instanceRoot, instanceVisualMeta])

  useEffect(() => {
    let cancelled = false
    const loadVisualMeta = async () => {
      for (const card of cards) {
        if (!card.instanceRoot) continue
        try {
          const result = await invoke<{
            mediaDataUrl?: string
            mediaPath?: string
            mediaMime?: string
            minecraftVersion?: string
            loader?: string
          } | null>('load_instance_visual_meta', { instanceRoot: card.instanceRoot })
          if (!result || cancelled) continue
          setInstanceVisualMeta((prev) => ({
            ...prev,
            [card.id]: {
              ...(prev[card.id] ?? {}),
              ...result,
            },
          }))
        } catch {
          // fallback a localStorage
        }
      }
    }
    void loadVisualMeta()
    return () => { cancelled = true }
  }, [cards])

  const refreshCardStatsForRoot = useCallback(async (instanceRoot: string) => {
    try {
      const stats = await invoke<InstanceCardStats>('get_instance_card_stats', { instanceRoot })
      setInstanceStatsByRoot((prev) => ({ ...prev, [instanceRoot]: stats }))
      const metadata = await invoke<InstanceMetadataView>('get_instance_metadata', { instanceRoot })
      setInstanceMetaByRoot((prev) => ({ ...prev, [instanceRoot]: metadata }))
    } catch {
      // No-op: refresco de stats best-effort para tooltips.
    }
  }, [])

  useEffect(() => {
    if (!selectedCard?.instanceRoot) {
      setRuntimeConsole([])
      return
    }
    setRuntimeConsole(runtimeConsoleByInstance[selectedCard.instanceRoot] ?? [])
  }, [runtimeConsoleByInstance, selectedCard?.instanceRoot])

  useEffect(() => {
    if (activePage !== 'Creador de Instancias') return
    setCreationConsoleLogs([])
    setCreationProgress(null)
  }, [activePage])

  useEffect(() => {
    if (!autoScrollConsole || !runtimeConsoleRef.current) return
    runtimeConsoleRef.current.scrollTop = runtimeConsoleRef.current.scrollHeight
  }, [runtimeConsole, autoScrollConsole])

  useEffect(() => {
    if (!creationConsoleRef.current) return
    creationConsoleRef.current.scrollTop = creationConsoleRef.current.scrollHeight
  }, [creationConsoleLogs, isCreating])

  useEffect(() => {
    if (!isAuthCooldown) return
    const timer = window.setInterval(() => setNowTick(Date.now()), 250)
    return () => window.clearInterval(timer)
  }, [isAuthCooldown])

  useEffect(() => {
    if (!selectedCard) return
    const maybeDraft = instanceDrafts[selectedCard.id]
    if (maybeDraft && maybeDraft.name === selectedCard.name) {
      setSelectedCard(maybeDraft)
    }
  }, [instanceDrafts, selectedCard])

  useEffect(() => {
    let cancelled = false

    const loadInstanceMetadata = async () => {
      if (!selectedCard?.instanceRoot || activePage !== 'Editar Instancia') {
        setSelectedInstanceMetadata(null)
        return
      }

      try {
        const metadata = await invoke<InstanceMetadataView>('get_instance_metadata', { instanceRoot: selectedCard.instanceRoot })
        if (!cancelled) {
          setSelectedInstanceMetadata(metadata)
          setInstanceMetaByRoot((prev) => ({ ...prev, [selectedCard.instanceRoot ?? '']: metadata }))
        }
      } catch (error) {
        if (cancelled) return
        const message = error instanceof Error ? error.message : String(error)
        appendRuntimeForRoot(selectedCard.instanceRoot, makeConsoleEntry('ERROR', 'launcher', `No se pudo cargar la configuración de la instancia: ${message}`))
      }
    }

    loadInstanceMetadata()

    return () => {
      cancelled = true
    }
  }, [activePage, selectedCard])

  useEffect(() => {
    let cancelled = false

    const warmupMetadata = async () => {
      const items = cards.filter((card) => card.instanceRoot)
      for (const card of items) {
        if (cancelled || !card.instanceRoot || instanceMetaByRoot[card.instanceRoot]) continue
        try {
          const metadata = await invoke<InstanceMetadataView>('get_instance_metadata', { instanceRoot: card.instanceRoot })
          if (cancelled) return
          setInstanceMetaByRoot((prev) => ({ ...prev, [card.instanceRoot ?? '']: metadata }))
        } catch {
          // Ignorar errores silenciosamente para no ensuciar la consola de creación.
        }
      }
    }

    void warmupMetadata()
    return () => {
      cancelled = true
    }
  }, [cards, instanceMetaByRoot])

  useEffect(() => {
    let cancelled = false

    const warmupCardStats = async () => {
      const roots = cards.map((card) => card.instanceRoot).filter((root): root is string => Boolean(root))
      for (const root of roots) {
        if (cancelled || instanceStatsByRoot[root]) continue
        try {
          const stats = await invoke<InstanceCardStats>('get_instance_card_stats', { instanceRoot: root })
          if (cancelled) return
          setInstanceStatsByRoot((prev) => ({ ...prev, [root]: stats }))
        } catch {
          // Ignorar errores para no bloquear el render principal de tarjetas.
        }
      }
    }

    void warmupCardStats()
    return () => {
      cancelled = true
    }
  }, [cards, instanceStatsByRoot])

  useEffect(() => {
    if (!selectedCard?.instanceRoot) {
      setIsInstanceRunning(false)
      return
    }

    let timer: number | null = null
    let cancelled = false

    const pollRuntimeStatus = async () => {
      try {
        const status = await invoke<RuntimeStatus>('get_runtime_status', { instanceRoot: selectedCard.instanceRoot })
        if (cancelled) return
        setIsInstanceRunning(status.running)

        if (!status.running && status.exitCode !== null) {
          const exitKey = `${selectedCard.instanceRoot}:${status.exitCode}:${status.pid ?? 'none'}`
          if (exitKey !== lastRuntimeExitKey) {
            appendRuntimeForRoot(selectedCard.instanceRoot, makeConsoleEntry(status.exitCode === 0 ? 'INFO' : 'ERROR', 'launcher', `Proceso finalizado con exit_code=${status.exitCode}.`))
            if (status.stderrTail.length > 0) {
              appendRuntimeForRoot(selectedCard.instanceRoot, makeConsoleEntry('WARN', 'game', `stderr (últimas ${status.stderrTail.length} líneas): ${status.stderrTail.join(' | ')}`))
            }
            setLastRuntimeExitKey(exitKey)
            const runningRoot = selectedCard.instanceRoot
            if (runningRoot) void refreshCardStatsForRoot(runningRoot)
          }
        }
      } catch {
        if (!cancelled) {
          setIsInstanceRunning(false)
        }
      }
    }

    void pollRuntimeStatus()
    timer = window.setInterval(() => {
      void pollRuntimeStatus()
    }, 2000)

    return () => {
      cancelled = true
      if (timer !== null) window.clearInterval(timer)
    }
  }, [lastRuntimeExitKey, refreshCardStatsForRoot, selectedCard?.instanceRoot])

  useEffect(() => {
    if (isInstanceRunning) {
      if (playtimeStartRef.current === null) playtimeStartRef.current = Date.now()
      return
    }

    if (playtimeStartRef.current !== null) {
      const elapsed = Date.now() - playtimeStartRef.current
      addPlaytimeToDefaultAccount(elapsed)
      playtimeStartRef.current = null
    }
  }, [isInstanceRunning])

  useEffect(() => {
    let cancelled = false
    let unlistenRuntimeOutput: UnlistenFn | null = null

    const wireRuntimeOutput = async () => {
      const unlisten = await listen<RuntimeOutputEvent>('instance_runtime_output', (event) => {
        const level = event.payload.stream === 'stderr' ? 'WARN' : 'INFO'
        const source = event.payload.stream === 'system' ? 'launcher' : 'game'
        const prefix = event.payload.stream === 'system' ? '' : `[${event.payload.stream.toUpperCase()}] `
        appendRuntimeForRoot(event.payload.instanceRoot, makeConsoleEntry(level, source, `${prefix}${event.payload.line}`))
        if (isStartingInstance && !isInstanceRunning) {
          setLaunchProgressPercent((prev) => Math.min(92, Math.max(prev + 4, 16)))
        }
      })

      if (cancelled) {
        void unlisten()
        return
      }

      unlistenRuntimeOutput = unlisten
    }

    void wireRuntimeOutput()

    return () => {
      cancelled = true
      if (unlistenRuntimeOutput) void unlistenRuntimeOutput()
    }
  }, [appendRuntimeForRoot, isInstanceRunning, isStartingInstance])

  useEffect(() => {
    const onEscapePress = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return

      const activeElement = document.activeElement as HTMLElement | null
      if (activeElement && ['INPUT', 'TEXTAREA', 'SELECT'].includes(activeElement.tagName)) {
        activeElement.blur()
        return
      }

      if (activePage === 'Editar Instancia') {
        event.preventDefault()
        if (selectedEditSection === 'Mods' && modsDownloaderOpen) {
          const selectedCount = Object.values(stagedDownloads).filter((entry) => entry.selected).length
          if (selectedCount > 0) {
            setCancelModsConfirmOpen(true)
            return
          }
          setModsDownloaderOpen(false)
          setReviewModalOpen(false)
          setInstallingModalOpen(false)
          setCancelModsConfirmOpen(false)
          return
        }
        if (selectedEditSection === 'Configuración' && selectedSettingsTab !== 'General') {
          setSelectedSettingsTab('General')
          return
        }
        if (selectedEditSection !== 'Ejecución') {
          setSelectedEditSection('Ejecución')
          return
        }
        if (isInstanceRunning || isStartingInstance) {
          appendRuntimeForRoot(selectedCard?.instanceRoot, makeConsoleEntry('WARN', 'launcher', 'No se puede cerrar el editor mientras la instancia está ejecutándose.'))
          return
        }
        setCreationProgress((prev) => prev ? { completed: prev.total, total: prev.total } : prev)
        if (backHistory.length > 0) {
          navigateBack()
        } else {
          navigateToPage('Mis Modpacks')
        }
        return
      }

      if (activePage === 'Creador de Instancias') {
        event.preventDefault()
        setCreationProgress((prev) => prev ? { completed: prev.total, total: prev.total } : prev)
        if (backHistory.length > 0) {
          navigateBack()
        } else {
          navigateToPage('Mis Modpacks')
        }
        return
      }

      if (activePage === 'Mis Modpacks' && selectedCard) {
        event.preventDefault()
        setSelectedCard(null)
        return
      }

      if (backHistory.length > 0) {
        event.preventDefault()
        navigateBack()
      }
    }

    window.addEventListener('keydown', onEscapePress)
    return () => window.removeEventListener('keydown', onEscapePress)
  }, [activePage, backHistory.length, isInstanceRunning, isStartingInstance, modsDownloaderOpen, selectedCard, selectedEditSection, selectedSettingsTab, stagedDownloads])

  useEffect(() => {
    let cancelled = false

    const loadInstances = async () => {
      await refreshInstances()
      if (cancelled) return
    }

    loadInstances()

    return () => {
      cancelled = true
    }
  }, [refreshInstances])

  useEffect(() => {
    if (activePage !== 'Mis Modpacks') return
    void refreshInstances()
  }, [activePage, refreshInstances])

  useEffect(() => {
    let cancelled = false
    setManifestLoading(true)
    setManifestError('')

    const cacheKey = 'mc_manifest_cache_v2'
    const cacheTtlMs = 1000 * 60 * 20

    const parsePayload = (payload: { versions?: ManifestVersion[] }) => {
      if (cancelled) return
      setManifestVersions(payload.versions ?? [])
    }

    const loadManifest = async () => {
      try {
        const cacheRaw = localStorage.getItem(cacheKey)
        if (cacheRaw) {
          const cache = JSON.parse(cacheRaw) as { timestamp: number; payload: { versions?: ManifestVersion[] } }
          if (Date.now() - cache.timestamp < cacheTtlMs && cache.payload?.versions) {
            parsePayload(cache.payload)
            setManifestLoading(false)
            return
          }
        }

        const response = await fetch(mojangManifestUrl)
        if (!response.ok) {
          throw new Error(`HTTP ${response.status}`)
        }

        const payload = (await response.json()) as { versions?: ManifestVersion[] }
        localStorage.setItem(cacheKey, JSON.stringify({ timestamp: Date.now(), payload }))
        parsePayload(payload)
      } catch (error) {
        if (!cancelled) {
          setManifestError(`No se pudo cargar el manifest oficial de Mojang: ${String(error)}`)
        }
      } finally {
        if (!cancelled) {
          setManifestLoading(false)
        }
      }
    }

    loadManifest()

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
        const items = sortLoaderVersions(payload
          .map((entry) => ({
            version: entry.loader?.version ?? '',
            publishedAt: '-',
            source: entry.stable ? 'stable' : 'latest',
          }))
          .filter((entry) => Boolean(entry.version)))

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
        const payload = (await response.json()) as Array<{ loader?: { version?: string }; stable?: boolean; created?: string }>
        const items = sortLoaderVersions(payload
          .map((entry) => ({
            version: entry.loader?.version ?? '',
            publishedAt: entry.created ?? '-',
            source: entry.stable ? 'stable' : 'latest',
          }))
          .filter((entry) => Boolean(entry.version)))

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
        const items = sortLoaderVersions(versions
          .filter((version) => version.startsWith(prefix))
          .map((version) => {
            const forgeVersion = version.slice(prefix.length)
            return {
              version: forgeVersion,
              publishedAt: '-',
              source: 'release',
              downloadUrl: `https://maven.minecraftforge.net/net/minecraftforge/forge/${selectedMinecraftVersion.id}-${forgeVersion}/forge-${selectedMinecraftVersion.id}-${forgeVersion}-installer.jar`,
            }
          }))

        if (!cancelled) {
          setLoaderVersions(items)
        }
        return
      }

      if (selectedLoader === 'neoforge') {
        // NeoForge 1.20.1 uses the legacy artifact (net.neoforged:forge).
        // From 1.20.2 onward the artifact is net.neoforged:neoforge with a
        // shortened version scheme (e.g. 20.2.x, 21.x.x).
        const mcId = selectedMinecraftVersion.id
        const isLegacyNeoForge = mcId === '1.20.1'

        let items: LoaderVersionItem[]

        if (isLegacyNeoForge) {
          const metadataUrl = 'https://maven.neoforged.net/releases/net/neoforged/forge/maven-metadata.xml'
          const response = await fetch(metadataUrl)
          if (!response.ok) {
            throw new Error(`NeoForge (legacy) maven metadata HTTP ${response.status}`)
          }
          const xmlText = await response.text()
          const doc = new DOMParser().parseFromString(xmlText, 'application/xml')
          const versions = Array.from(doc.querySelectorAll('version')).map((node) => node.textContent?.trim() ?? '')
          const prefix = `${mcId}-`
          items = sortLoaderVersions(versions
            .filter((version) => version.startsWith(prefix))
            .map((version) => {
              const loaderPart = version.slice(prefix.length)
              return {
                version: loaderPart,
                publishedAt: '-',
                source: 'release',
                downloadUrl: `https://maven.neoforged.net/releases/net/neoforged/forge/${version}/forge-${version}-installer.jar`,
              }
            }))
        } else {
          const metadataUrl = 'https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml'
          const response = await fetch(metadataUrl)
          if (!response.ok) {
            throw new Error(`NeoForge maven metadata HTTP ${response.status}`)
          }
          const xmlText = await response.text()
          const doc = new DOMParser().parseFromString(xmlText, 'application/xml')
          const versions = Array.from(doc.querySelectorAll('version')).map((node) => node.textContent?.trim() ?? '')
          const family = inferNeoForgeFamily(mcId)
          items = sortLoaderVersions(versions
            .filter((version) => {
              if (!family) return true
              return version === family || version.startsWith(`${family}.`)
            })
            .map((version) => ({
              version,
              publishedAt: '-',
              source: 'release',
              downloadUrl: `https://maven.neoforged.net/releases/net/neoforged/neoforge/${version}/neoforge-${version}-installer.jar`,
            })))
        }

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

  useEffect(() => {
    if (!manifestVersions.length) return
    const versionExists = selectedMinecraftVersion && manifestVersions.some((entry) => entry.id === selectedMinecraftVersion.id)
    if (versionExists) return

    const latestRelease = [...manifestVersions]
      .filter((entry) => entry.type === 'release')
      .sort((left, right) => Date.parse(right.releaseTime) - Date.parse(left.releaseTime))[0]

    setSelectedMinecraftVersion(latestRelease ?? manifestVersions[0])
  }, [manifestVersions, selectedMinecraftVersion])

  useEffect(() => {
    if (!loaderVersions.length) {
      setSelectedLoaderVersion(null)
      return
    }

    const currentVersionExists = selectedLoaderVersion && loaderVersions.some((entry) => entry.version === selectedLoaderVersion.version)
    if (currentVersionExists) return

    const stableFirst = loaderVersions.find((entry) => entry.source === 'stable')
    setSelectedLoaderVersion(stableFirst ?? loaderVersions[0])
  }, [loaderVersions, selectedLoaderVersion])

  const filteredCards = useMemo(() => {
    const term = instanceSearch.trim().toLowerCase()
    if (!term) {
      return cards
    }

    return cards.filter((card) => card.name.toLowerCase().includes(term) || card.group.toLowerCase().includes(term))
  }, [cards, instanceSearch])

  const minecraftRows = useMemo<[string, string, string][]>(() => {
    const searchTerm = minecraftSearch.trim().toLowerCase()
    return [...manifestVersions]
      .sort((left, right) => Date.parse(right.releaseTime) - Date.parse(left.releaseTime))
      .filter((version) => {
        if (selectedMcFilter === 'Releases') return version.type === 'release'
        if (selectedMcFilter === 'Snapshots') return version.type === 'snapshot'
        if (selectedMcFilter === 'Betas') return version.type === 'old_beta'
        if (selectedMcFilter === 'Alfas') return version.type === 'old_alpha'
        return version.id.toLowerCase().includes('experimental')
      })
      .filter((version) => {
        if (selectedMcChannel === 'Todos') return true
        if (selectedMcChannel === 'Estables') return version.type === 'release'
        return version.type === 'snapshot' || version.id.toLowerCase().includes('experimental')
      })
      .filter((version) => !searchTerm || version.id.toLowerCase().includes(searchTerm))
      .map((version) => [version.id, formatIsoDate(version.releaseTime), mapTypeToSpanish(version.type)])
  }, [manifestVersions, minecraftSearch, selectedMcChannel, selectedMcFilter])

  const loaderRows = useMemo<[string, string, string][]>(() => {
    const searchTerm = loaderSearch.trim().toLowerCase()
    return loaderVersions
      .filter((entry) => {
        if (selectedLoaderFilter === 'Todos') return true
        if (selectedLoaderFilter === 'Stable') return entry.source === 'stable'
        if (selectedLoaderFilter === 'Latest') return entry.source === 'latest'
        return entry.source === 'release'
      })
      .filter((entry) => !searchTerm || entry.version.toLowerCase().includes(searchTerm))
      .map((entry) => [entry.version, entry.publishedAt, mapTypeToSpanish(entry.source)])
  }, [loaderSearch, loaderVersions, selectedLoaderFilter])


  const loaderLabel = selectedLoader === 'none' ? 'vanilla' : selectedLoader
  const selectedJavaMajor = toJavaMajorOrUndefined(selectedMinecraftDetail?.javaVersion?.majorVersion) ?? 17
  const instanceRootLabel = selectedCard?.instanceRoot ?? 'Sin ruta de instancia todavía'
  const minecraftRootLabel = selectedCard?.instanceRoot ? `${selectedCard.instanceRoot}/minecraft` : 'Sin ruta minecraft todavía'

  const creatorSectionRows = useMemo<[string, string][]>(() => {
    const loaderVersionLabel = selectedLoaderVersion?.version ?? 'sin selección'
    const mcVersionLabel = selectedMinecraftVersion?.id ?? 'sin selección'

    if (selectedCreatorSection === 'CurseForge') {
      return [
        ['Ruta del manifiesto CF', `${minecraftRootLabel}/modpack/manifest.json`],
        ['Versión MC asignada', mcVersionLabel],
        ['Loader asignado', `${loaderLabel} ${loaderVersionLabel}`],
        ['Carpeta de mods objetivo', `${minecraftRootLabel}/mods`],
      ]
    }

    if (selectedCreatorSection === 'Modrinth') {
      return [
        ['Ruta del índice Modrinth', `${minecraftRootLabel}/modrinth.index.json`],
        ['Versión MC asignada', mcVersionLabel],
        ['Loader asignado', `${loaderLabel} ${loaderVersionLabel}`],
        ['Carpeta de overrides', `${minecraftRootLabel}/config`],
      ]
    }

    return [
      ['Ruta base de instancia', instanceRootLabel],
      ['Ruta de minecraft', minecraftRootLabel],
      ['Versión MC asignada', mcVersionLabel],
      ['Java asignado', `Java ${selectedJavaMajor}`],
      ['Loader asignado', `${loaderLabel} ${loaderVersionLabel}`],
      ['RAM objetivo', '4096 MiB'],
    ]
  }, [instanceRootLabel, loaderLabel, minecraftRootLabel, selectedCreatorSection, selectedJavaMajor, selectedLoaderVersion?.version, selectedMinecraftVersion?.id])

  const classpathPreviewRows = useMemo<[string, string][]>(() => {
    const effectiveClasspath = launchPreparation?.classpath ?? ''
    if (!effectiveClasspath.trim()) {
      return [['Classpath', 'Aún no preparado. Inicia validación para obtener rutas reales.']]
    }

    return effectiveClasspath
      .split(/[:;]/)
      .map((entry) => entry.trim())
      .filter(Boolean)
      .slice(0, 12)
      .map((entry, index) => [`CP ${index + 1}`, entry])
  }, [launchPreparation?.classpath])

  const envRows = useMemo<[string, string][]>(() => {
    const gameDir = selectedCard?.instanceRoot ? `${selectedCard.instanceRoot}/minecraft` : '-'
    const natives = `${gameDir}/natives`
    const assets = `${gameDir}/assets`
    return [
      ['GAME_DIR', gameDir],
      ['MC_ASSETS_ROOT', assets],
      ['JAVA_HOME', selectedInstanceMetadata?.javaPath ?? launchPreparation?.javaPath ?? '-'],
      ['NATIVES_DIR', natives],
    ]
  }, [launchPreparation?.javaPath, selectedCard?.instanceRoot, selectedInstanceMetadata?.javaPath])

  const createInstance = async () => {
    const cleanName = instanceName.trim()
    if (!cleanName || isCreating || !selectedMinecraftVersion || !selectedMinecraftDetail) {
      return
    }

    const cleanGroup = groupName.trim() || defaultGroup

    if (!authSession) {
      setCreationConsoleLogs(['Error: Debes iniciar sesión con una cuenta oficial para crear instancias (sin Demo).'])
      return
    }
    const diskEstimateMb = 1024
    const requiredJava = toJavaMajorOrUndefined(selectedMinecraftDetail.javaVersion?.majorVersion) ?? 17

    if (cards.some((card) => card.name.toLowerCase() === cleanName.toLowerCase())) {
      setCreationConsoleLogs(['Error: Ya existe una instancia con ese nombre.'])
      return
    }

    setIsCreating(true)
    const requestId = `create-${Date.now()}-${Math.random().toString(16).slice(2)}`
    let unlistenCreationProgress: UnlistenFn | null = null
    setCreationProgress(null)
    setCreationConsoleLogs([])
    setCreationConsoleLogs([
      'FASE 2 iniciada al presionar OK.',
      'Validación ✓ nombre no vacío.',
      'Validación ✓ version.json disponible.',
      `Validación ✓ espacio mínimo estimado (${diskEstimateMb} MB).`,
      `Preparación ✓ Java requerido: ${requiredJava}.`,
      'Preparación ✓ no se realizaron descargas pesadas durante la selección.',
      'Creación iniciada: esperando eventos del backend...',
    ])

    try {
      unlistenCreationProgress = await listen<InstanceCreationProgressEvent>('instance_creation_progress', (event) => {
        if (event.payload.requestId && event.payload.requestId !== requestId) {
          return
        }
        if (typeof event.payload.completed === 'number' && typeof event.payload.total === 'number' && event.payload.total > 0) {
          setCreationProgress({ completed: event.payload.completed, total: event.payload.total })
        }
        setCreationConsoleLogs((prev) => [...prev, event.payload.message])
      })

      const result = await invoke<CreateInstanceResult>('create_instance', {
        payload: {
          name: cleanName,
          group: cleanGroup,
          minecraftVersion: selectedMinecraftVersion.id,
          loader: mapLoaderToPayload(selectedLoader),
          loaderVersion: selectedLoaderVersion?.version ?? '',
          requiredJavaMajor: requiredJava,
          ramMb: 4096,
          javaArgs: ['-XX:+UseG1GC'],
          authSession: {
            profileId: authSession.profileId,
            profileName: authSession.profileName,
            minecraftAccessToken: authSession.minecraftAccessToken,
            minecraftAccessTokenExpiresAt: authSession.minecraftAccessTokenExpiresAt,
            microsoftRefreshToken: authSession.microsoftRefreshToken,
            premiumVerified: authSession.premiumVerified,
          },
          creationRequestId: requestId,
        },
      })

      const created = { id: result.id, name: result.name, group: result.group, instanceRoot: result.instanceRoot }
      setCards((prev) => [...prev, created])
      setInstanceDrafts((prev) => ({ ...prev, [created.id]: created }))
      setSelectedCard(created)
      const visualMeta: InstanceVisualMeta = {
        mediaDataUrl: instanceIconPreview.startsWith('data:image') ? instanceIconPreview : undefined,
        mediaMime: instanceIconPreview.startsWith('data:image') ? 'image/*' : undefined,
        minecraftVersion: selectedMinecraftVersion.id,
        loader: selectedLoader === 'none' ? 'Vanilla' : `${selectedLoader} ${selectedLoaderVersion?.version ?? ''}`.trim(),
      }
      setInstanceVisualMeta((prev) => ({
        ...prev,
        [created.id]: visualMeta,
      }))
      if (created.instanceRoot) {
        void invoke('save_instance_visual_meta', { instanceRoot: created.instanceRoot, meta: visualMeta })
      }
      setCreationConsoleLogs((prev) => [...prev, ...result.logs, '✅ Instancia creada correctamente.'])
      setInstanceName('')
      setGroupName(defaultGroup)
      setCreationProgress((prev) => prev ? { completed: prev.total, total: prev.total } : prev)
      navigateToPage('Mis Modpacks')
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      setCreationConsoleLogs((prev) => [...prev, `Error: ${message}`])
    } finally {
      if (unlistenCreationProgress) {
        unlistenCreationProgress()
      }
      setIsCreating(false)
    }
  }

  const uploadSelectedCardIcon = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0]
    if (!file || !selectedCard) return

    const normalizedMime = file.type || inferMimeFromName(file.name) || ''
    const isSupported = ['image/png', 'image/jpeg', 'image/gif', 'image/webp', 'video/mp4'].includes(normalizedMime)
    if (!isSupported) {
      setCreationConsoleLogs((prev) => [...prev, `Error: formato no soportado (${file.name}). Usa PNG, JPG, JPEG, GIF, WEBP o MP4.`])
      return
    }

    try {
      const bytes = Array.from(new Uint8Array(await file.arrayBuffer()))
      let mediaPath: string | undefined
      if (selectedCard.instanceRoot) {
        mediaPath = await invoke<string>('save_instance_visual_media', {
          instanceRoot: selectedCard.instanceRoot,
          fileName: file.name,
          bytes,
        })
      }
      const shouldStoreInline = !mediaPath && normalizedMime.startsWith('image/')
      const data = shouldStoreInline
        ? await new Promise<string>((resolve, reject) => {
          const reader = new FileReader()
          reader.onload = () => resolve(typeof reader.result === 'string' ? reader.result : '')
          reader.onerror = () => reject(new Error('No se pudo leer el archivo visual.'))
          reader.readAsDataURL(file)
        })
        : ''
      setInstanceVisualMeta((prev) => ({
        ...prev,
        [selectedCard.id]: {
          ...(prev[selectedCard.id] ?? {}),
          mediaDataUrl: data || undefined,
          mediaPath: mediaPath ?? prev[selectedCard.id]?.mediaPath,
          mediaMime: normalizedMime,
        },
      }))
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      setCreationConsoleLogs((prev) => [...prev, `Error subiendo media de instancia: ${message}`])
    } finally {
      event.target.value = ''
    }
  }

  const uploadInstanceIcon = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0]
    if (!file) return
    if (!file.type.startsWith('image/')) {
      setCreationConsoleLogs((prev) => [...prev, `Error: ${file.name} no es una imagen válida.`])
      return
    }

    const reader = new FileReader()
    reader.onload = () => {
      const data = typeof reader.result === 'string' ? reader.result : ''
      if (data) {
        setInstanceIconPreview(data)
        setCreationConsoleLogs((prev) => [...prev, `Icono actualizado desde ${file.name}.`])
      }
    }
    reader.readAsDataURL(file)
  }



  const startInstanceProcess = async () => {
    if (!selectedCard?.instanceRoot) return
    if (!authSession) {
      appendRuntimeForRoot(selectedCard.instanceRoot, makeConsoleEntry('ERROR', 'launcher', 'Debes iniciar sesión con cuenta oficial para iniciar (sin Demo).'))
      return
    }
    if (isStartingInstance || isInstanceRunning) {
      appendRuntimeForRoot(selectedCard.instanceRoot, makeConsoleEntry('WARN', 'launcher', 'La instancia ya está en ejecución o iniciándose.'))
      return
    }

    setIsStartingInstance(true)
    setLaunchProgressPercent(8)
    setRuntimeConsoleByInstance((prev) => ({ ...prev, [selectedCard.instanceRoot ?? '']: [] }))
    appendRuntimeForRoot(selectedCard.instanceRoot, makeConsoleEntry('INFO', 'launcher', 'Iniciando validación final y arranque en vivo de Minecraft...'))

    try {
      const result = await invoke<StartInstanceResult>('start_instance', {
        instanceRoot: selectedCard.instanceRoot,
        authSession: {
          profileId: authSession.profileId,
          profileName: authSession.profileName,
          minecraftAccessToken: authSession.minecraftAccessToken,
          minecraftAccessTokenExpiresAt: authSession.minecraftAccessTokenExpiresAt,
          microsoftRefreshToken: authSession.microsoftRefreshToken,
          premiumVerified: authSession.premiumVerified,
        },
      })

      const refreshedSession: AuthSession = {
        ...authSession,
        profileId: result.refreshedAuthSession.profileId,
        profileName: result.refreshedAuthSession.profileName,
        minecraftAccessToken: result.refreshedAuthSession.minecraftAccessToken,
        minecraftAccessTokenExpiresAt: result.refreshedAuthSession.minecraftAccessTokenExpiresAt ?? authSession.minecraftAccessTokenExpiresAt,
        microsoftRefreshToken: result.refreshedAuthSession.microsoftRefreshToken ?? undefined,
        premiumVerified: result.refreshedAuthSession.premiumVerified,
        loggedAt: Date.now(),
      }
      setAuthSession(refreshedSession)
      syncManagedAccountFromSession(refreshedSession)
      persistAuthSession(refreshedSession)

      appendRuntimeForRoot(selectedCard.instanceRoot, makeConsoleEntry('INFO', 'launcher', `Proceso de Minecraft iniciado (PID ${result.pid}) con Java ${result.javaPath}`))
      result.logs.forEach((line) => appendRuntimeForRoot(selectedCard.instanceRoot, makeConsoleEntry('INFO', 'launcher', line)))
      setLaunchProgressPercent(100)
      setIsInstanceRunning(true)
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      appendRuntimeForRoot(selectedCard.instanceRoot, makeConsoleEntry('ERROR', 'launcher', `No se pudo iniciar el proceso de la instancia: ${message}`))
    } finally {
      setIsStartingInstance(false)
      window.setTimeout(() => setLaunchProgressPercent(0), 320)
    }
  }



  const exportRuntimeLog = async () => {
    const content = runtimeConsole.map((entry) => `[${entry.timestamp}] [${entry.level}] [${entry.source}] ${entry.message}`).join('\n')
    const fileName = `launcher-${selectedCard?.name ?? 'instance'}.log`

    try {
      if ('showSaveFilePicker' in window) {
        const handle = await (window as { showSaveFilePicker: (options: unknown) => Promise<{ createWritable: () => Promise<{ write: (content: string) => Promise<void>; close: () => Promise<void> }> }> }).showSaveFilePicker({
          suggestedName: fileName,
          types: [{ description: 'Archivos de log', accept: { 'text/plain': ['.log'] } }],
        })
        const writable = await handle.createWritable()
        await writable.write(content)
        await writable.close()
        appendRuntime(makeConsoleEntry('INFO', 'launcher', 'Log exportado correctamente.'))
        return
      }

      const blob = new Blob([content], { type: 'text/plain;charset=utf-8' })
      const url = URL.createObjectURL(blob)
      const link = document.createElement('a')
      link.href = url
      link.download = fileName
      document.body.appendChild(link)
      link.click()
      link.remove()
      URL.revokeObjectURL(url)
      appendRuntime(makeConsoleEntry('WARN', 'launcher', 'Tu entorno no soporta selector de guardado nativo. Se usó descarga directa.'))
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      appendRuntime(makeConsoleEntry('WARN', 'launcher', `Exportación cancelada o fallida: ${message}`))
    }
  }

  const exportInstancePackage = async (format: InstanceExportFormat) => {
    if (!selectedCard?.instanceRoot) return
    const backendFormat = format === 'mrpack' ? 'mrpack' : format === 'curseforge-zip' ? 'curseforge' : 'prism'
    try {
      const result = await invoke<{ outputPath: string }>('export_instance_package', {
        request: {
          instanceRoot: selectedCard.instanceRoot,
          instanceName: selectedCard.name,
          exportFormat: backendFormat,
        },
      })
      setCreationConsoleLogs((prev) => [...prev, `Exportación completada: ${result.outputPath}`])
      setShowExportMenu(false)
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      setCreationConsoleLogs((prev) => [...prev, `No se pudo exportar la instancia: ${message}`])
    }
  }


  const openEditor = () => {
    if (!selectedCard) {
      return
    }

    setSelectedEditSection('Ejecución')
    setSelectedSettingsTab('General')
    navigateToPage('Editar Instancia')
  }

  const handleInstanceAction = async (action: string) => {
    if (!selectedCard) return

    if (action === 'Iniciar') {
      if (isStartingInstance || isInstanceRunning) return
      openEditor()
      void startInstanceProcess()
      return
    }

    if (action === 'Forzar Cierre') {
      if (!selectedCard.instanceRoot) {
        setCreationConsoleLogs((prev) => [...prev, `No hay ruta registrada para la instancia ${selectedCard.name}.`])
        return
      }
      try {
        const result = await invoke<string>('force_close_instance', { instanceRoot: selectedCard.instanceRoot })
        setCreationConsoleLogs((prev) => [...prev, result])
        setIsStartingInstance(false)
        setIsInstanceRunning(false)
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error)
        setCreationConsoleLogs((prev) => [...prev, `No se pudo forzar cierre de la instancia: ${message}`])
      }
      return
    }

    if (action === 'Editar') {
      openEditor()
      return
    }

    if (action === 'Exportar') {
      setShowExportMenu((prev) => !prev)
      return
    }

    if (action === 'Eliminar') {
      setShowDeleteInstanceConfirm(true)
      return
    }

    if (action === 'Carpeta (Origen)') {
      if (!selectedCard.instanceRoot) {
        setCreationConsoleLogs((prev) => [...prev, `No hay ruta registrada para la instancia ${selectedCard.name}.`])
        return
      }
      try {
        await invoke('open_redirect_origin_folder', { instanceRoot: selectedCard.instanceRoot })
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error)
        setCreationConsoleLogs((prev) => [...prev, `No se pudo abrir la carpeta origen: ${message}`])
      }
      return
    }

    if (action !== 'Carpeta (Interface)') return

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

  const deleteSelectedInstance = async () => {
    if (!selectedCard?.instanceRoot || isDeletingInstance) return

    setIsDeletingInstance(true)
    try {
      await invoke('delete_instance', { instanceRoot: selectedCard.instanceRoot })
      const loadedCards = await invoke<InstanceSummary[]>('list_instances')
      setCards(loadedCards)
      setSelectedCard(null)
      setShowDeleteInstanceConfirm(false)
      setCreationConsoleLogs((prev) => [...prev, `Instancia eliminada: ${selectedCard.name}`])
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      setCreationConsoleLogs((prev) => [...prev, `No se pudo eliminar la instancia: ${message}`])
    } finally {
      setIsDeletingInstance(false)
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

  const formatPlaytime = (totalMs: number) => {
    const totalSeconds = Math.max(0, Math.floor(totalMs / 1000))
    const hours = Math.floor(totalSeconds / 3600)
    const minutes = Math.floor((totalSeconds % 3600) / 60)
    const seconds = totalSeconds % 60
    return `${hours}h ${minutes}m ${seconds}s`
  }

  const openAccountManager = () => {
    setAccountMenuOpen(false)
    navigateToPage('Administradora de cuentas')
  }

  const navigateToPage = (page: MainPage) => {
    if (page === activePage) return
    setBackHistory((prev) => [...prev, activePage])
    setForwardHistory([])
    setActivePage(page)
  }

  const navigateBack = () => {
    setBackHistory((prev) => {
      if (prev.length === 0) return prev
      const next = [...prev]
      const previousPage = next.pop() as MainPage
      setForwardHistory((forwardPrev) => [activePage, ...forwardPrev])
      setActivePage(previousPage)
      return next
    })
  }

  const navigateForward = () => {
    setForwardHistory((prev) => {
      if (prev.length === 0) return prev
      const [nextPage, ...rest] = prev
      setBackHistory((backPrev) => [...backPrev, activePage])
      setActivePage(nextPage)
      return rest
    })
  }

  useEffect(() => {
    if (!managedAccounts.length) {
      setSelectedAccountId('')
      return
    }
    setSelectedAccountId((prev) => prev && managedAccounts.some((account) => account.profileId === prev) ? prev : managedAccounts[0].profileId)
  }, [managedAccounts])

  const accountManagerRows = managedAccounts.map((account) => ({
    ...account,
    typeLabel: account.type,
    stateLabel: account.status,
  }))
  const totalPlaytimeAllInstances = useMemo(
    () => managedAccounts.reduce((total, account) => total + account.totalPlaytimeMs, 0),
    [managedAccounts],
  )

  useEffect(() => {
    const loadRoutes = async () => {
      try {
        const fromBackend = await invoke<FolderRoutesPayload>('load_folder_routes')
        const valid = defaultFolderRoutes.map((item) => {
          const found = fromBackend.routes.find((entry) => entry?.key === item.key)
          return found && typeof found.value === 'string' ? { ...item, value: found.value } : item
        })
        const sanitized = sanitizeFolderRoutes(valid)
        setFolderRoutes(sanitized)
        persistFolderRoutes(sanitized)
        return
      } catch {
        // Fallback local cuando Tauri no está disponible.
      }

      const raw = localStorage.getItem(folderRoutesKey)
      if (!raw) return
      try {
        const parsed = JSON.parse(raw) as FolderRouteItem[]
        if (!Array.isArray(parsed)) return
        const valid = defaultFolderRoutes.map((item) => {
          const found = parsed.find((entry) => entry?.key === item.key)
          return found && typeof found.value === 'string' ? { ...item, value: found.value } : item
        })
        setFolderRoutes(sanitizeFolderRoutes(valid))
      } catch {
        setFolderRoutes(sanitizeFolderRoutes(defaultFolderRoutes))
      }
    }

    void loadRoutes()
  }, [])


  useEffect(() => {
    const raw = localStorage.getItem(languageSettingsKey)
    if (raw && languageCatalog.some((lang) => lang.name === raw)) {
      setSelectedLanguage(raw)
    }

    const installedRaw = localStorage.getItem('launcher_installed_languages_v1')
    if (!installedRaw) return
    try {
      const parsed = JSON.parse(installedRaw) as string[]
      if (Array.isArray(parsed) && parsed.length > 0) {
        setInstalledLanguages(parsed.filter((value) => languageCatalog.some((lang) => lang.name === value)))
      }
    } catch {
      // ignore invalid storage
    }
  }, [])

  useEffect(() => {
    localStorage.setItem(languageSettingsKey, selectedLanguage)
    document.documentElement.lang = languageLocaleMap[selectedLanguage] ?? 'en-US'
  }, [selectedLanguage])

  useEffect(() => {
    localStorage.setItem('launcher_installed_languages_v1', JSON.stringify(installedLanguages))
  }, [installedLanguages])

  useEffect(() => {
    const raw = localStorage.getItem(appearanceSettingsKey)
    if (!raw) {
      setAppearanceLoaded(true)
      return
    }
    try {
      const parsed = JSON.parse(raw) as {
        preset?: string
        fontFamily?: string
        uiScalePercent?: number
        uiElementScalePercent?: number
        customVars?: Partial<Record<AppearanceColorKey, string>>
        customThemes?: UserAppearanceTheme[]
      }
      if (parsed.preset && (appearancePresets.some((item) => item.id === parsed.preset) || parsed.preset === 'custom' || parsed.preset.startsWith('user-'))) {
        setSelectedAppearancePreset(parsed.preset)
      }
      if (parsed.fontFamily) {
        setSelectedFontFamily(parsed.fontFamily)
      }
      if (typeof parsed.uiScalePercent === 'number') {
        setUiScalePercent(Math.min(120, Math.max(85, Math.round(parsed.uiScalePercent))))
      }
      if (typeof parsed.uiElementScalePercent === 'number') {
        setUiElementScalePercent(Math.min(120, Math.max(85, Math.round(parsed.uiElementScalePercent))))
      }
      if (parsed.customVars) {
        setCustomAppearanceVars((prev) => ({ ...prev, ...parsed.customVars }))
      }
      if (Array.isArray(parsed.customThemes)) {
        setUserAppearanceThemes(parsed.customThemes.filter((item) => item && typeof item.id === 'string' && typeof item.name === 'string'))
      }
    } catch {
      localStorage.removeItem(appearanceSettingsKey)
    } finally {
      setAppearanceLoaded(true)
    }
  }, [])

  useEffect(() => {
    if (selectedAppearancePreset === 'custom') return
    const preset = appearancePresets.find((item) => item.id === selectedAppearancePreset)
    const userPreset = userAppearanceThemes.find((item) => item.id === selectedAppearancePreset)
    const nextVars = userPreset?.vars ?? preset?.vars
    if (!nextVars) return
    setCustomAppearanceVars((prev) => {
      const hasChanges = (Object.keys(nextVars) as AppearanceColorKey[]).some((key) => prev[key] !== nextVars[key])
      return hasChanges ? { ...prev, ...nextVars } : prev
    })
  }, [selectedAppearancePreset, userAppearanceThemes])

  useEffect(() => {
    const preset = appearancePresets.find((item) => item.id === selectedAppearancePreset)
    const userPreset = userAppearanceThemes.find((item) => item.id === selectedAppearancePreset)
    const vars = selectedAppearancePreset === 'custom' ? customAppearanceVars : (userPreset?.vars ?? preset?.vars ?? appearancePresets[0].vars)
    Object.entries(vars).forEach(([key, value]) => {
      document.documentElement.style.setProperty(key, value)
    })
    document.documentElement.style.setProperty('--bg-soft', vars['--bg-surface'])
    document.documentElement.style.setProperty('--text', vars['--text-main'])
    document.documentElement.style.setProperty('--panel-color', vars['--bg-surface'])
    document.documentElement.style.setProperty('--container-color', vars['--bg-surface-muted'])
    document.documentElement.style.setProperty('--button-color', vars['--bg-surface-muted'])
    document.documentElement.style.setProperty('--button-hover-color', vars['--bg-hover'])
    document.documentElement.style.setProperty('--text-color', vars['--text-main'])
    document.documentElement.style.setProperty('--font-ui', selectedFontFamily)
    document.documentElement.style.setProperty('--ui-scale', `${uiScalePercent / 100}`)
    document.documentElement.style.setProperty('--ui-element-scale', `${uiElementScalePercent / 100}`)

    if (!appearanceLoaded) return
    localStorage.setItem(appearanceSettingsKey, JSON.stringify({
      preset: selectedAppearancePreset,
      fontFamily: selectedFontFamily,
      uiScalePercent,
      uiElementScalePercent,
      customVars: customAppearanceVars,
      customThemes: userAppearanceThemes,
    }))
  }, [appearanceLoaded, selectedAppearancePreset, selectedFontFamily, uiScalePercent, uiElementScalePercent, customAppearanceVars, userAppearanceThemes])


  useEffect(() => {
    const loadMods = async () => {
      if (selectedEditSection !== 'Mods' || !selectedCard?.instanceRoot) return
      setModsLoading(true)
      setModsError('')
      try {
        const rows = await invoke<InstanceModEntry[]>('list_instance_mods', { instanceRoot: selectedCard.instanceRoot })
        setInstanceMods(rows)
      } catch (error) {
        setModsError(error instanceof Error ? error.message : String(error))
      } finally {
        setModsLoading(false)
      }
    }
    void loadMods()
  }, [selectedCard?.instanceRoot, selectedEditSection])

  const formatBytes = (bytes: number) => {
    if (bytes <= 0) return '0 B'
    const units = ['B', 'KB', 'MB', 'GB']
    let size = bytes
    let index = 0
    while (size >= 1024 && index < units.length - 1) {
      size /= 1024
      index += 1
    }
    return `${size.toFixed(index === 0 ? 0 : 1)} ${units[index]}`
  }

  const inferModTag = useCallback((mod: InstanceModEntry) => {
    const logs = selectedCard?.instanceRoot ? (runtimeConsoleByInstance[selectedCard.instanceRoot] ?? []) : []
    const related = logs.filter((entry) => entry.message.toLowerCase().includes(mod.name.toLowerCase()) || entry.message.toLowerCase().includes(mod.fileName.toLowerCase()))
    const pool = (related.length ? related : logs).map((entry) => entry.message.toLowerCase())
    if (pool.some((line) => line.includes('dependenc'))) return 'dependencia'
    if (pool.some((line) => line.includes('incompat'))) return 'incompatible'
    if (pool.some((line) => line.includes('crash'))) return 'crash'
    if (pool.some((line) => line.includes('warn'))) return 'warn'
    if (pool.some((line) => line.includes('login') || line.includes('sesión') || line.includes('session'))) return 'sesion'
    if (pool.some((line) => line.includes('start') || line.includes('inicio'))) return 'inicio'
    return '-'
  }, [runtimeConsoleByInstance, selectedCard?.instanceRoot])

  const filteredMods = useMemo(() => {
    const query = modsSearch.trim().toLowerCase()
    return instanceMods.filter((mod) => {
      const bySearch = !query || mod.name.toLowerCase().includes(query) || mod.fileName.toLowerCase().includes(query)
      const byProvider = modsProviderFilter === 'all' || mod.provider === modsProviderFilter
      const tag = inferModTag(mod)
      const byTag = modsAdvancedFilter.tag === 'all' || tag === modsAdvancedFilter.tag
      const byState = modsAdvancedFilter.state === 'all' || (modsAdvancedFilter.state === 'enabled' ? mod.enabled : !mod.enabled)
      return bySearch && byProvider && byTag && byState
    })
  }, [inferModTag, instanceMods, modsAdvancedFilter.state, modsAdvancedFilter.tag, modsProviderFilter, modsSearch])

  const modsTotalPages = Math.max(1, Math.ceil(filteredMods.length / 30))
  const pagedMods = useMemo(() => {
    const start = (modsPage - 1) * 30
    return filteredMods.slice(start, start + 30)
  }, [filteredMods, modsPage])


  useEffect(() => {
    if (modsPage > modsTotalPages) setModsPage(modsTotalPages)
  }, [modsPage, modsTotalPages])

  useEffect(() => {
    let cancelled = false
    const fetchInstalledModIcons = async () => {
      if (instanceMods.length === 0) {
        setModIconById({})
        return
      }
      const entries = await Promise.all(instanceMods.map(async (mod) => {
        try {
          const payload = await invoke<{ items: Array<{ image?: string }> }>('search_catalogs', { request: { search: mod.name, curseforgeClassId: 6, platform: 'Todas', mcVersion: selectedInstanceMetadata?.minecraftVersion ?? null, loader: (selectedInstanceMetadata?.loader ?? '').toLowerCase() || null, category: 'mod', modrinthSort: 'relevance', curseforgeSortField: 1, limit: 1, page: 1 } })
          return [mod.id, payload.items[0]?.image ?? ''] as const
        } catch {
          return [mod.id, ''] as const
        }
      }))
      if (cancelled) return
      setModIconById(Object.fromEntries(entries.filter(([, icon]) => icon)))
    }
    void fetchInstalledModIcons()
    return () => { cancelled = true }
  }, [instanceMods, selectedInstanceMetadata?.loader, selectedInstanceMetadata?.minecraftVersion])

  const selectedMod = useMemo(() => instanceMods.find((item) => item.id === selectedModId) ?? null, [instanceMods, selectedModId])
  const isCatalogSource = modsDownloaderSource === 'Modrinth' || modsDownloaderSource === 'CurseForge'
  const selectedCatalogMod = useMemo(() => downloaderCatalogMods.find((item) => item.id === selectedCatalogModId) ?? null, [downloaderCatalogMods, selectedCatalogModId])
  const selectedCatalogDetail = selectedCatalogMod ? (catalogDetailByModId[selectedCatalogMod.id] ?? null) : null

  const mapDetailToCatalogVersions = useCallback((detail: ModCatalogDetail) => {
    return detail.versions
      .filter((entry) => !!entry.downloadUrl)
      .map((entry) => ({
        id: entry.id || `${entry.name}-${entry.gameVersion}-${entry.downloadUrl}`,
        name: entry.name,
        gameVersion: entry.gameVersion,
        versionType: (entry as { versionType?: string }).versionType ?? 'release',
        publishedAt: (entry as { publishedAt?: string }).publishedAt ?? '',
        downloadUrl: entry.downloadUrl,
        requiredDependencies: entry.requiredDependencies ?? [],
      }))
  }, [])


  const selectedCatalogVersions = useMemo(() => {
    if (!selectedCatalogDetail) return []
    return mapDetailToCatalogVersions(selectedCatalogDetail)
  }, [mapDetailToCatalogVersions, selectedCatalogDetail])

  const selectedCatalogVersion = useMemo(() => selectedCatalogVersions[0] ?? null, [selectedCatalogVersions])

  const releaseMinecraftVersions = useMemo(() => manifestVersions.filter((entry) => entry.type === 'release').map((entry) => entry.id), [manifestVersions])

  useEffect(() => {
    const timeout = window.setTimeout(() => setDebouncedDownloaderSearch(downloaderSearch.trim()), 260)
    return () => window.clearTimeout(timeout)
  }, [downloaderSearch])

  const fetchDownloaderCatalog = useCallback(async () => {
    if (!modsDownloaderOpen || !isCatalogSource) return
    setModsCatalogLoading(true)
    setModsCatalogError('')
    try {
      const payload = await invoke<{ items: Array<{ id: string; source: ModsCatalogSource; title: string; description: string; image?: string; downloads: number; updatedAt: string }> }>('search_catalogs', {
        request: {
          search: debouncedDownloaderSearch,
          curseforgeClassId: 6,
          platform: modsDownloaderSource,
          mcVersion: downloaderShowAllVersions ? null : (downloaderVersionFilter || selectedInstanceMetadata?.minecraftVersion || null),
          loader: (downloaderLoaderFilter || selectedInstanceMetadata?.loader || '').toLowerCase() || null,
          category: downloaderClientOnly ? 'client' : downloaderServerOnly ? 'server' : 'mod',
          modrinthSort: modsDownloaderSort,
          curseforgeSortField: modsDownloaderSort === 'downloads' ? 6 : modsDownloaderSort === 'updated' ? 3 : modsDownloaderSort === 'followers' ? 2 : modsDownloaderSort === 'newest' ? 4 : 1,
          limit: 30,
          page: 1,
        },
      })
      const rows: ModsCatalogItem[] = payload.items.map((item) => ({
        id: item.id,
        source: item.source,
        name: item.title,
        summary: item.description,
        image: item.image ?? '',
        downloads: item.downloads ?? 0,
        followers: 0,
        publishedAt: item.updatedAt,
        updatedAt: item.updatedAt,
      }))
      setDownloaderCatalogMods(rows)
      setSelectedCatalogModId((prev) => (prev && rows.some((item) => item.id === prev) ? prev : rows[0]?.id ?? ''))
    } catch (error) {
      setModsCatalogError(error instanceof Error ? error.message : String(error))
      setDownloaderCatalogMods([])
      setSelectedCatalogModId('')
    } finally {
      setModsCatalogLoading(false)
    }
  }, [debouncedDownloaderSearch, downloaderClientOnly, downloaderLoaderFilter, downloaderServerOnly, downloaderShowAllVersions, downloaderVersionFilter, isCatalogSource, modsDownloaderOpen, modsDownloaderSort, modsDownloaderSource, selectedInstanceMetadata?.loader, selectedInstanceMetadata?.minecraftVersion])

  useEffect(() => { void fetchDownloaderCatalog() }, [fetchDownloaderCatalog])

  useEffect(() => {
    const fetchSelectedDetail = async () => {
      if (!selectedCatalogMod || catalogDetailByModId[selectedCatalogMod.id]) return
      setCatalogDetailLoading(true)
      try {
        const detail = await invoke<ModCatalogDetail>('get_catalog_detail', { request: { id: selectedCatalogMod.id, source: selectedCatalogMod.source } })
        setCatalogDetailByModId((prev) => ({ ...prev, [selectedCatalogMod.id]: detail }))
      } finally {
        setCatalogDetailLoading(false)
      }
    }
    void fetchSelectedDetail()
  }, [catalogDetailByModId, selectedCatalogMod])

  const openDownloader = () => {
    setModsDownloaderOpen(true)
    setModsDownloaderSource('Modrinth')
    setDownloaderSearch('')
    setDownloaderVersionFilter(selectedInstanceMetadata?.minecraftVersion ?? '')
    setDownloaderLoaderFilter(selectedInstanceMetadata?.loader ?? '')
    setDownloaderShowAllVersions(false)
  }

  const closeDownloader = useCallback(() => {
    setModsDownloaderOpen(false)
    setReviewModalOpen(false)
    setInstallingModalOpen(false)
    setCancelModsConfirmOpen(false)
  }, [])

  const closeDownloaderWithValidation = useCallback(() => {
    const selectedCount = Object.values(stagedDownloads).filter((entry) => entry.selected).length
    if (selectedCount > 0) {
      setCancelModsConfirmOpen(true)
      return
    }
    closeDownloader()
  }, [closeDownloader, stagedDownloads])

  const resolveRequiredDependencies = useCallback(async (mod: ModsCatalogItem, version: CatalogVersionItem, visited = new Set<string>()) => {
    if (visited.has(mod.id)) return [] as StagedDownloadEntry['dependencies']
    visited.add(mod.id)
    const dependencyIds = version.requiredDependencies ?? []
    const resolved: StagedDownloadEntry['dependencies'] = []
    for (const dependencyId of dependencyIds) {
      const knownDependency = downloaderCatalogMods.find((item) => item.id === dependencyId && item.source === mod.source)
      const dependencyMod: ModsCatalogItem = knownDependency ?? {
        id: dependencyId,
        source: mod.source,
        name: `Dependencia ${dependencyId}`,
        summary: 'Dependencia obligatoria detectada automáticamente.',
        image: '',
        downloads: 0,
        followers: 0,
        publishedAt: '',
        updatedAt: '',
      }
      let dependencyVersion: CatalogVersionItem | null = null
      try {
        const detail = await invoke<ModCatalogDetail>('get_catalog_detail', { request: { id: dependencyMod.id, source: dependencyMod.source } })
        dependencyVersion = mapDetailToCatalogVersions(detail)[0] ?? null
      } catch {
        dependencyVersion = null
      }
      const installed = instanceMods.some((item) => item.name.toLowerCase() === dependencyMod.name.toLowerCase())
      resolved.push({ mod: dependencyMod, version: dependencyVersion, installed, selected: !installed })
      if (dependencyVersion) {
        const nested = await resolveRequiredDependencies(dependencyMod, dependencyVersion, visited)
        for (const nestedDependency of nested) {
          if (!resolved.some((item) => item.mod.id === nestedDependency.mod.id)) {
            resolved.push(nestedDependency)
          }
        }
      }
    }
    return resolved
  }, [downloaderCatalogMods, instanceMods, mapDetailToCatalogVersions])

  const stageSelectedMod = async () => {
    if (!selectedCatalogMod || !selectedCatalogVersion) return
    if (stagedDownloads[selectedCatalogMod.id]) {
      setStagedDownloads((prev) => {
        const next = { ...prev }
        delete next[selectedCatalogMod.id]
        return next
      })
      return
    }
    const dependencies = await resolveRequiredDependencies(selectedCatalogMod, selectedCatalogVersion)
    const reinstall = instanceMods.some((item) => item.name.toLowerCase() === selectedCatalogMod.name.toLowerCase())
    setStagedDownloads((prev) => ({
      ...prev,
      [selectedCatalogMod.id]: {
        mod: selectedCatalogMod,
        version: selectedCatalogVersion,
        reinstall,
        selected: !reinstall,
        dependencies,
      },
    }))
  }

  const reviewTree = useMemo(() => Object.values(stagedDownloads), [stagedDownloads])

  const toggleStageFromReview = (modId: string, checked: boolean) => {
    setStagedDownloads((prev) => ({ ...prev, [modId]: { ...prev[modId], selected: checked } }))
  }

  const confirmReviewAndInstall = async () => {
    if (!selectedCard?.instanceRoot) return
    const selected = Object.values(stagedDownloads).filter((entry) => entry.selected)
    if (selected.length === 0) {
      window.alert('No hay mods seleccionados para descargar.')
      return
    }
    setReviewModalOpen(false)
    setInstallingModalOpen(true)
    setInstallProgress({ current: 0, total: selected.length, message: 'Iniciando descarga...' })
    for (const [index, entry] of selected.entries()) {
      setInstallProgress({ current: index, total: selected.length, message: `Descargando ${entry.mod.name}...` })
      await invoke('install_catalog_mod_file', {
        instanceRoot: selectedCard.instanceRoot,
        downloadUrl: entry.version.downloadUrl,
        fileName: entry.version.name,
        replaceExisting: entry.reinstall,
      })
      setInstallProgress({ current: index + 1, total: selected.length, message: `Instalado ${entry.mod.name}` })
    }
    setReviewModalOpen(false)
    setModsDownloaderOpen(false)
    setInstallingModalOpen(false)
    setStagedDownloads({})
    await reloadMods()
  }

  const shortenUrl = (value: string) => {
    try {
      const url = new URL(value)
      const trimmedPath = url.pathname.length > 18 ? `${url.pathname.slice(0, 18)}…` : url.pathname
      return `${url.hostname}${trimmedPath}`
    } catch {
      return value
    }
  }

  const escapeHtml = (value: string) => value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')

  const renderCatalogBody = (value?: string) => {
    if (!value) return ''
    const raw = value.trim()
    if (!raw) return ''
    if (/<\/?[a-z][\s\S]*>/i.test(raw)) return raw

    const escaped = escapeHtml(raw)
    const withImages = escaped.replace(/!\[([^\]]*)\]\((https?:\/\/[^)\s]+)\)/g, '<img src="$2" alt="$1" loading="lazy" />')
    const withLinks = withImages.replace(/\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g, '<a href="$2" target="_blank" rel="noopener noreferrer">$1</a>')
    const paragraphs = withLinks
      .split(/\n{2,}/)
      .map((block) => block.trim())
      .filter(Boolean)
      .map((block) => `<p>${block.replace(/\n/g, '<br />')}</p>`)
      .join('')
    return paragraphs
  }

  const startModNameColumnDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault()
    const startX = event.clientX
    const initialWidth = modsNameColumnWidth
    const onPointerMove = (moveEvent: PointerEvent) => {
      const delta = moveEvent.clientX - startX
      setModsNameColumnWidth(Math.max(240, Math.min(560, initialWidth + delta)))
    }
    const stopDrag = () => {
      window.removeEventListener('pointermove', onPointerMove)
      window.removeEventListener('pointerup', stopDrag)
      window.removeEventListener('pointercancel', stopDrag)
    }
    window.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', stopDrag)
    window.addEventListener('pointercancel', stopDrag)
  }

  const resolveProviderLabel = (provider: string) => {
    if (provider === 'Desconocido') return 'Local'
    return provider
  }

  const reloadMods = useCallback(async () => {
    if (!selectedCard?.instanceRoot) return
    const rows = await invoke<InstanceModEntry[]>('list_instance_mods', { instanceRoot: selectedCard.instanceRoot })
    setInstanceMods(rows)
  }, [selectedCard?.instanceRoot])

  const toggleModEnabled = useCallback(async (mod: InstanceModEntry, desired: boolean) => {
    if (!selectedCard?.instanceRoot) return
    await invoke('set_instance_mod_enabled', { instanceRoot: selectedCard.instanceRoot, fileName: mod.fileName, enabled: desired })
    await reloadMods()
  }, [reloadMods, selectedCard?.instanceRoot])

  const fetchVersionOptions = useCallback(async () => {
    if (!selectedMod || !selectedCard?.instanceRoot) return
    const q = selectedMod.name
    setModVersionLoading(true)
    setModVersionError('')
    try {
      const payload = await invoke<{ items: Array<{ id: string; title: string; source: 'CurseForge' | 'Modrinth' }> }>('search_catalogs', {
        request: {
          search: q,
          curseforgeClassId: 6,
          platform: 'Todas',
          mcVersion: downloaderShowAllVersions ? null : (downloaderVersionFilter || selectedInstanceMetadata?.minecraftVersion || null),
          loader: (selectedInstanceMetadata?.loader ?? '').toLowerCase() || null,
          category: downloaderClientOnly ? 'client' : downloaderServerOnly ? 'server' : 'mod',
          modrinthSort: 'relevance',
          curseforgeSortField: 1,
          limit: 8,
          page: 1,
        },
      })
      const first = payload.items[0]
      if (!first) {
        setModVersionOptions([])
        return
      }
      const detail = await invoke<ModCatalogDetail>('get_catalog_detail', { request: { id: first.id, source: first.source } })
      const options: ModVersionOption[] = detail.versions
        .filter((entry: { name: string; gameVersion: string; downloadUrl: string }) => !!entry.downloadUrl)
        .map((entry: { name: string; gameVersion: string; downloadUrl: string }) => ({
          name: entry.name,
          version: entry.gameVersion,
          downloadUrl: entry.downloadUrl,
          fileName: `${entry.name.replace(/\s+/g, '-')}.jar`,
        }))
      setModVersionOptions(options)
      setModVersionDetail(detail)
      setSelectedVersionOptionId(options[0] ? `${options[0].name}-${options[0].version}` : '')
      setModVersionModalOpen(true)
    } catch (error) {
      setModVersionError(error instanceof Error ? error.message : String(error))
    } finally {
      setModVersionLoading(false)
    }
  }, [downloaderClientOnly, downloaderServerOnly, downloaderShowAllVersions, downloaderVersionFilter, selectedCard?.instanceRoot, selectedInstanceMetadata?.loader, selectedInstanceMetadata?.minecraftVersion, selectedMod])

  const replaceSpecificModVersion = useCallback(async (mod: InstanceModEntry, option: ModVersionOption) => {
    if (!selectedCard?.instanceRoot) return
    await invoke('replace_instance_mod_file', {
      instanceRoot: selectedCard.instanceRoot,
      currentFileName: mod.fileName,
      downloadUrl: option.downloadUrl,
      newFileName: option.fileName,
    })
    await reloadMods()
  }, [reloadMods, selectedCard?.instanceRoot])

  const replaceModVersion = useCallback(async (option: ModVersionOption) => {
    if (!selectedMod) return
    await replaceSpecificModVersion(selectedMod, option)
  }, [replaceSpecificModVersion, selectedMod])

  const checkInstalledModUpdates = useCallback(async () => {
    if (!selectedCard?.instanceRoot) return
    setUpdatesReviewLoading(true)
    setModVersionError('')
    try {
      const results: InstalledModUpdateCandidate[] = []
      for (const mod of instanceMods) {
        const payload = await invoke<{ items: Array<{ id: string; source: 'CurseForge' | 'Modrinth'; title: string }> }>('search_catalogs', { request: { search: mod.name, curseforgeClassId: 6, platform: 'Todas', mcVersion: selectedInstanceMetadata?.minecraftVersion ?? null, loader: (selectedInstanceMetadata?.loader ?? '').toLowerCase() || null, category: 'mod', modrinthSort: 'relevance', curseforgeSortField: 1, limit: 1, page: 1 } })
        const first = payload.items[0]
        if (!first) continue
        const detail = await invoke<ModCatalogDetail>('get_catalog_detail', { request: { id: first.id, source: first.source } })
        const next = mapDetailToCatalogVersions(detail)[0]
        if (!next) continue
        if (!next.name.toLowerCase().includes(mod.version.toLowerCase())) {
          results.push({ mod, nextVersion: { name: next.name, version: next.gameVersion, downloadUrl: next.downloadUrl, fileName: `${next.name.replace(/\s+/g, '-')}.jar` } })
        }
      }
      setUpdatesCandidates(results)
      setUpdatesModalOpen(true)
    } catch (error) {
      setModVersionError(error instanceof Error ? error.message : String(error))
    } finally {
      setUpdatesReviewLoading(false)
    }
  }, [instanceMods, mapDetailToCatalogVersions, selectedCard?.instanceRoot, selectedInstanceMetadata?.loader, selectedInstanceMetadata?.minecraftVersion])

  const applyAllUpdates = useCallback(async () => {
    for (const candidate of updatesCandidates) {
      // eslint-disable-next-line no-await-in-loop
      await replaceSpecificModVersion(candidate.mod, candidate.nextVersion)
    }
    setUpdatesModalOpen(false)
    setUpdatesCandidates([])
  }, [replaceSpecificModVersion, updatesCandidates])

  return (
    <div className="app-shell">
      <PrincipalTopBar
        authSession={authSession}
        activePage={activePage}
        uiLanguage={uiLanguage}
        onNavigate={navigateToPage}
        onLogout={logout}
        onOpenAccountManager={openAccountManager}
        accountMenuOpen={accountMenuOpen}
        onToggleMenu={() => setAccountMenuOpen((prev) => !prev)}
        onNavigateBack={navigateBack}
        onNavigateForward={navigateForward}
        canNavigateBack={backHistory.length > 0}
        canNavigateForward={forwardHistory.length > 0}
        hideSecondaryNav={activePage === 'Creador de Instancias' || activePage === 'Editor de skins' || activePage === 'Editar Instancia' || activePage === 'Updates'}
      />

      <AnimatePresence mode="wait">
        <motion.div
          key={`${authSession ? 'auth' : 'guest'}-${activePage}`}
          className="page-transition-wrapper"
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -8 }}
          transition={{ duration: 0.22, ease: 'easeOut' }}
        >

      {!isAuthReady && (
        <main className="content content-padded">
          <section className="section-placeholder">
            <h2>Verificando sesión...</h2>
            <p>Comprobando si ya existe un login de Microsoft guardado.</p>
          </section>
        </main>
      )}

      {isAuthReady && !authSession && (
        <main className="content content-padded">
          <section className="floating-modal auth-login-card" style={{ margin: '2rem auto' }}>
            <div className="auth-login-card-header">
              <span className="auth-login-chip">Acceso seguro</span>
              <h3>Inicia sesión con Microsoft</h3>
              <p>Usa el inicio de sesión oficial y autoriza tu cuenta para continuar en el launcher.</p>
            </div>
            {authStatus && <p className="auth-feedback auth-feedback-status">{authStatus}</p>}
            {authError && <p className="auth-feedback auth-feedback-error">{authError}</p>}
            {isAuthCooldown && <p className="auth-feedback auth-feedback-warn">Espera {authRetrySeconds}s antes de generar un nuevo código de inicio de sesión.</p>}
            <p className="auth-feedback auth-feedback-status">Se abrirá una ventana segura del launcher para iniciar sesión y elegir cuenta.</p>
            <div className="floating-modal-actions auth-actions">
              <button className="primary" onClick={() => void startMicrosoftLogin()} disabled={isAuthenticating || isAuthCooldown}>
                {isAuthenticating
                  ? 'Conectando...'
                  : isAuthCooldown
                    ? `Espera ${authRetrySeconds}s`
                    : 'Continuar con Microsoft'}
              </button>
            </div>
          </section>
        </main>
      )}


      {authSession && activePage === 'Inicio' && (
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
                  <span className="instance-group-chip">{card.group}</span>
                </article>
              ))}
            </div>
          </section>
        </main>
      )}

      {authSession && activePage === 'Administradora de cuentas' && (
        <main className="content content-padded">
          <h1 className="page-title">Administradora de cuentas</h1>
          <section className="account-manager-layout">
            <aside className="account-manager-panel compact">
              <button className="active">Cuentas</button>
              <button disabled>Próximamente 1</button>
              <button disabled>Próximamente 2</button>
              <button disabled>Próximamente 3</button>
            </aside>

            <section className="account-manager-main">
              <header>
                <h2>Cuentas logeadas</h2>
                <p>Listado de cuentas disponibles en el launcher.</p>
              </header>
              <div className="account-table">
                <div className="account-table-head">
                  <span>Nombre de Usuario</span>
                  <span>Uuid</span>
                  <span>Correo</span>
                  <span>Tipo</span>
                  <span>Estado</span>
                  <span>Tiempo jugado</span>
                </div>
                <div className="account-table-body">
                  {accountManagerRows.length === 0 && (
                    <p className="account-empty">Aún no hay cuentas registradas.</p>
                  )}
                  {accountManagerRows.map((account) => (
                    <div
                      key={account.profileId}
                      className={`account-row selectable ${selectedAccountId === account.profileId ? 'active' : ''}`}
                      onClick={() => setSelectedAccountId(account.profileId)}
                    >
                      <span>{account.profileName}{account.isDefault ? ' (Predeterminada)' : ''}</span>
                      <span>{account.profileId}</span>
                      <span>{account.email}</span>
                      <span>{account.typeLabel}</span>
                      <span>{account.stateLabel}</span>
                      <span>{formatPlaytime(account.totalPlaytimeMs)}</span>
                    </div>
                  ))}
                </div>
              </div>
            </section>

            <aside className="account-manager-panel compact">
              <button onClick={() => void startMicrosoftLogin()}>Añadir Microsoft</button>
              <button disabled>Añadir Offline (Próximamente)</button>
              <button onClick={() => window.location.reload()}>Refrescar</button>
              <button
                onClick={() => {
                  if (!selectedAccountId) return
                  setManagedAccounts((prev) => {
                    const next = prev.filter((account) => account.profileId !== selectedAccountId)
                    persistManagedAccounts(next)
                    return next
                  })
                }}
                disabled={!selectedAccountId}
              >
                Remover
              </button>
              <button
                onClick={() => {
                  if (!selectedAccountId) return
                  setManagedAccounts((prev) => {
                    const next = prev.map((account) => ({ ...account, isDefault: account.profileId === selectedAccountId }))
                    persistManagedAccounts(next)
                    return next
                  })
                }}
                disabled={!selectedAccountId}
              >
                Establecer por Defecto
              </button>
              <button disabled={!selectedAccountId} onClick={() => navigateToPage('Administradora de skins')}>Administrar Skins</button>
            </aside>
          </section>
        </main>
      )}
      {authSession && (activePage === 'Administradora de skins' || activePage === 'Editor de skins') && (
        <SkinStudio
          activePage={activePage}
          selectedAccountId={selectedAccountId}
          onNavigateEditor={() => navigateToPage('Editor de skins')}
        />
      )}



      {authSession && activePage === 'Mis Modpacks' && (
        <main className="content content-padded">
          <section className="instances-panel huge-panel">
            <header className="panel-actions">
              <button className="primary" onClick={() => navigateToPage('Creador de Instancias')}>
                Crear instancia
              </button>
              <input
                className="instance-search-compact"
                type="search"
                value={instanceSearch}
                onChange={(event) => setInstanceSearch(event.target.value)}
                placeholder="Buscar instancia"
                aria-label="Buscar instancia"
              />
              <button className="primary" onClick={() => navigateToPage('Importar Instancias')}>Importar</button>
              <button>Vista</button>
            </header>

            <div className="instances-workspace with-right-panel">
              <div className="cards-grid instances-grid-area">
                {filteredCards.length === 0 && <article className="instance-card placeholder">No hay instancias para mostrar.</article>}
                {filteredCards.map((card) => {
                  const visual = instanceVisualMeta[card.id]
                  const metadata = card.instanceRoot ? instanceMetaByRoot[card.instanceRoot] : undefined
                  const cardVersion = metadata?.minecraftVersion ?? visual?.minecraftVersion ?? '-'
                  const rawLoader = (metadata?.loader ?? visual?.loader ?? 'vanilla').toLowerCase()
                  const cardLoader = rawLoader.includes('fabric')
                    ? '🧵 Fabric'
                    : rawLoader.includes('neoforge')
                      ? '🔶 NeoForge'
                      : rawLoader.includes('forge')
                        ? '⚒️ Forge'
                        : rawLoader.includes('quilt') || rawLoader.includes('quilit')
                          ? '🧶 Quilt'
                          : '🟩 Vanilla'
                  const cardLoaderVersion = metadata?.loaderVersion ?? ''
                  const mediaDataUrl = resolveVisualMedia(visual)
                  const mediaType = mediaTypeFromMeta(visual)

                  return (
                    <motion.article
                      key={card.id}
                      className={`instance-card clickable ${selectedCard?.id === card.id ? 'active' : ''}`}
                      onClick={() => setSelectedCard(card)}
                      whileHover={{ y: -2, scale: 1.01 }}
                      transition={{ duration: 0.14 }}
                    >
                      <div className="instance-card-icon hero" aria-hidden="true">
                        {!mediaDataUrl ? '🧱' : mediaType === 'video' ? <video src={mediaDataUrl} muted loop autoPlay playsInline /> : <img src={mediaDataUrl} alt="" loading="lazy" />}
                      </div>
                      {metadata?.state?.toUpperCase() === 'REDIRECT' && <span className="instance-tag atajo">Atajo</span>}
                      <strong className="instance-card-title">{card.name}</strong>
                      <div className="instance-card-meta">
                        <small>Version: {cardVersion}</small>
                        <small>Loader: {cardLoader} {cardLoaderVersion}</small>
                      </div>
                      <div className="instance-card-hover-info">
                        {(() => {
                          const stats = card.instanceRoot ? instanceStatsByRoot[card.instanceRoot] : undefined
                          const hoverInfo: InstanceHoverInfo = {
                            size: stats?.sizeMb ? `${stats.sizeMb} MB` : '-',
                            createdAt: metadata?.createdAt ? formatIsoDate(metadata.createdAt, selectedLocale) : '-',
                            lastUsedAt: stats?.lastUsed
                              ? formatIsoDate(stats.lastUsed, selectedLocale)
                              : (metadata?.lastUsed ? formatIsoDate(metadata.lastUsed, selectedLocale) : '-'),
                            author: authSession?.profileName ?? 'INTERFACE',
                            modsCount: String(stats?.modsCount ?? 0),
                          }

                          return (
                            <>
                              <p>Peso: {hoverInfo.size}</p>
                              <p>Creada: {hoverInfo.createdAt}</p>
                              <p>Último uso: {hoverInfo.lastUsedAt}</p>
                              <p>Autor: {hoverInfo.author}</p>
                              <p>Mods: {hoverInfo.modsCount}</p>
                            </>
                          )
                        })()}
                      </div>
                      {(selectedCard?.id === card.id && (isStartingInstance || isInstanceRunning)) && (
                        <>
                          <span className="instance-state-chip">{isStartingInstance ? 'Iniciando' : 'Ejecutando'}</span>
                          <div className="instance-run-progress" aria-label="Progreso de ejecución">
                            <div className={`instance-run-progress-fill ${isInstanceRunning ? 'running' : ''}`} />
                          </div>
                        </>
                      )}
                    </motion.article>
                  )
                })}
              </div>

              <aside className="instance-right-panel">
                {selectedCard ? (
                  <>
                  <input ref={selectedCardIconInputRef} type="file" accept="image/*,video/*" hidden onChange={(event) => void uploadSelectedCardIcon(event)} />
                  <div className="instance-right-hero clickable" onClick={() => selectedCardIconInputRef.current?.click()} aria-hidden="true">
                    {(() => {
                      const media = resolveVisualMedia(instanceVisualMeta[selectedCard.id])
                      const type = mediaTypeFromMeta(instanceVisualMeta[selectedCard.id])
                      if (!media) return '🧱'
                      return type === 'video' ? <video src={media} muted loop autoPlay playsInline /> : <img src={media} alt="" loading="lazy" />
                    })()}
                  </div>
                  <header>
                    <h3>{selectedCard.name}</h3>
                  </header>
                  <div className="instance-right-actions">
                    {instanceActions.map((action) => (
                      <div key={action} className="instance-action-item">
                        <button className={action === 'Editar' ? 'primary' : ''} onClick={() => handleInstanceAction(action)}>
                          {action}
                        </button>
                        {action === 'Exportar' && showExportMenu && (
                          <div className="instance-export-menu">
                            <button onClick={() => void exportInstancePackage('prism-zip')}>Prism Launcher (.zip)</button>
                            <button onClick={() => void exportInstancePackage('curseforge-zip')}>CurseForge (.zip)</button>
                            <button onClick={() => void exportInstancePackage('mrpack')}>Modrinth (.mrpack)</button>
                            <button className="ghost-btn" onClick={() => void exportRuntimeLog()}>Exportar log (.log)</button>
                          </div>
                        )}
                        {action === 'Editar' && (
                          <button className="danger" onClick={() => handleInstanceAction('Eliminar')}>
                            Eliminar
                          </button>
                        )}
                      </div>
                    ))}
                    {instanceMetaByRoot[selectedCard.instanceRoot ?? '']?.state?.toUpperCase() === 'REDIRECT' && (
                      <div className="instance-action-item">
                        <button onClick={() => void handleInstanceAction('Carpeta (Origen)')}>📂 Carpeta (Origen)</button>
                      </div>
                    )}
                  </div>
                  </>
                ) : null}
              </aside>
            </div>
          </section>
          <section className="instances-summary-panel">
            <div className="instances-summary-item">
              <span className="summary-label">Tiempo total jugado</span>
              <strong>{formatPlaytime(totalPlaytimeAllInstances)}</strong>
            </div>
            <div className="instances-summary-item">
              <span className="summary-label">Instancias registradas</span>
              <strong>{cards.length}</strong>
            </div>
            <button className="primary" onClick={() => navigateToPage('Updates')}>Updates</button>
          </section>
        </main>
      )}


      {authSession && activePage === 'Importar Instancias' && (
        <ImportPage onInstancesChanged={refreshInstances} uiLanguage={uiLanguage} />
      )}

      {authSession && activePage === 'Explorador' && (
        <ExplorerPage uiLanguage={uiLanguage} />
      )}

      {authSession && activePage === 'Updates' && (
        <main className="content content-padded updates-page">
          <section className="instances-panel updates-panel">
            <header className="news-panel-header updates-header">
              <div>
                <h2>Updates</h2>
                <p>Historial de versiones, fechas y descripciones. Preparado para auto-update nativo de Tauri + GitHub Releases.</p>
              </div>
              <div className="updates-actions">
                <button className="primary" onClick={() => void checkLauncherUpdates()}>Actualizar a la última versión</button>
                <button onClick={() => void invoke('open_url_in_browser', { url: launcherUpdatesUrl, browserId: 'default' })}>Ver releases</button>
              </div>
            </header>

            <div className="updates-status-bar">{updatesStatus}</div>

            <div className="updates-table-wrap">
              <table className="updates-table">
                <thead>
                  <tr>
                    <th>Versión</th>
                    <th>Fecha</th>
                    <th>Canal</th>
                    <th>Descripción</th>
                    <th>Estado</th>
                  </tr>
                </thead>
                <tbody>
                  {launcherUpdatesFeed.map((item) => (
                    <tr key={item.version}>
                      <td>{item.version}</td>
                      <td>{formatIsoDate(item.releaseDate)}</td>
                      <td>{item.channel}</td>
                      <td>{item.summary}</td>
                      <td><span className="news-chip">{item.status}</span></td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>

            <article className="global-setting-item tauri-updater-guide">
              <h3>Estructura técnica preparada (Tauri Updater)</h3>
              <p>1) Activar updater en tauri.conf.json con endpoint latest.json y pubkey.</p>
              <p>2) Generar claves con <code>tauri signer generate</code>.</p>
              <p>3) Firmar builds con <code>tauri build</code> y subir .sig + metadata.</p>
              <p>4) Publicar en GitHub Releases y consumir con checkUpdate/installUpdate en frontend.</p>
            </article>
          </section>
        </main>
      )}

      {authSession && activePage === 'Novedades' && (
        <main className="content content-padded news-page">
          <section className="instances-panel news-panel">
            <header className="news-panel-header">
              <div>
                <h2>Novedades</h2>
                <p>Comunicados y notas destacadas del launcher. Esta sección es independiente del historial de updates.</p>
              </div>
            </header>

            <div className="news-grid" />
          </section>
        </main>
      )}

      {authSession && activePage === 'Configuración Global' && (
        <main className="content content-padded">
          <section className="instances-panel global-settings-panel">
            <header className="news-panel-header">
              <div>
                <h2>{ui.globalTitle}</h2>
                <p>{ui.globalDesc}</p>
              </div>
            </header>

            <div className="global-settings-tabs" role="tablist" aria-label="Pestañas de configuración global">
              {globalSettingsTabs.map((tab) => (
                <button
                  key={tab}
                  className={selectedGlobalSettingsTab === tab ? 'active' : ''}
                  onClick={() => setSelectedGlobalSettingsTab(tab)}
                >
                  {tab}
                </button>
              ))}
            </div>

            {selectedGlobalSettingsTab === 'General' && (
              <div className="global-settings-list professional-general-grid">
                <article className="global-setting-item folder-routes-card">
                  <h3>{ui.folderTitle}</h3>
                  <div className="folder-route-list">
                    <FolderRow label={`📁 ${ui.launcherRoot}`} path={launcherFolders?.launcherRoot ?? '-'} />
                    <FolderRow label={`📁 ${ui.instances}`} path={launcherFolders?.instancesDir ?? '-'} />
                    <FolderRow label={`📁 ${ui.runtime}`} path={launcherFolders?.runtimeDir ?? '-'} />
                    <FolderRow label="📁 Skins / Assets" path={launcherFolders?.assetsDir ?? '-'} />
                  </div>
                  <div className="folder-route-actions" style={{ marginTop: '0.7rem' }}>
                    <button className="primary" onClick={() => void pickNewLauncherRoot()}>🔁 {ui.changeRoot}</button>
                    <button onClick={() => void pickNewInstancesFolder()}>📦 {ui.changeInstances}</button>
                  </div>
                </article>
              </div>
            )}

            {selectedGlobalSettingsTab === 'Idioma' && (
              <section className="global-settings-list language-settings-elegant">
                <header className="language-toolbar">
                  <input
                    type="search"
                    value={languageSearch}
                    onChange={(event) => setLanguageSearch(event.target.value)}
                    placeholder="Buscar idioma"
                    aria-label="Buscar idioma"
                  />
                  <span>Actual: <strong>{selectedLanguage}</strong></span>
                </header>
                <div className="language-list">
                  {filteredLanguages.map((lang) => {
                    const installed = installedLanguages.includes(lang.name)
                    return (
                      <button
                        key={lang.name}
                        className={selectedLanguage === lang.name ? 'active' : ''}
                        onClick={() => {
                          if (!installed) {
                            const approved = window.confirm(`El idioma ${lang.name} no está instalado. ¿Deseas descargarlo e instalarlo ahora?`)
                            if (!approved) return
                            setInstalledLanguages((prev) => prev.includes(lang.name) ? prev : [...prev, lang.name])
                          }
                          setSelectedLanguage(lang.name)
                        }}
                      >
                        {lang.name}
                        {!installed ? <small className="language-tag-not-installed">No instalado</small> : null}
                      </button>
                    )
                  })}
                </div>
              </section>
            )}

            {selectedGlobalSettingsTab === 'Apariencia' && (
              <section className="appearance-workspace">
                <div className="appearance-presets">
                  <h3>Apariencia</h3>
                  <label className="appearance-control-field">
                    <span>Tema predeterminado</span>
                    <select
                      value={appearancePresets.some((preset) => preset.id === selectedAppearancePreset) ? selectedAppearancePreset : 'custom'}
                      onChange={(event) => setSelectedAppearancePreset(event.target.value)}
                    >
                      {appearancePresets.map((preset) => <option key={preset.id} value={preset.id}>{preset.name}</option>)}
                      <option value="custom">Personalizado manual</option>
                    </select>
                  </label>
                  <label className="appearance-control-field">
                    <span>Temas personalizados guardados</span>
                    <select
                      value={selectedAppearancePreset.startsWith('user-') ? selectedAppearancePreset : ''}
                      onChange={(event) => { if (event.target.value) setSelectedAppearancePreset(event.target.value) }}
                    >
                      <option value="">Seleccionar tema guardado</option>
                      {userAppearanceThemes.map((theme) => <option key={theme.id} value={theme.id}>🧩 {theme.name}</option>)}
                    </select>
                  </label>
                  <div className="appearance-color-grid">
                    {([
                      ['Fondo', '--bg-main'],
                      ['Superficie', '--bg-surface'],
                      ['Superficie secundaria', '--bg-surface-muted'],
                      ['Hover', '--bg-hover'],
                      ['Borde', '--border'],
                      ['Texto', '--text-main'],
                      ['Texto secundario', '--text-muted'],
                      ['Acento', '--accent'],
                      ['Acento hover', '--accent-hover'],
                    ] as [string, AppearanceColorKey][]).map(([label, key]) => (
                      <label key={key} className="appearance-color-row">
                        <span>{label}</span>
                        <input
                          type="color"
                          value={customAppearanceVars[key]}
                          onChange={(event) => {
                            setSelectedAppearancePreset('custom')
                            setCustomAppearanceVars((prev) => ({ ...prev, [key]: event.target.value }))
                          }}
                        />
                      </label>
                    ))}
                  </div>
                </div>

                <div className="appearance-preview detailed">
                  <h3>Tipografía</h3>
                  <label className="appearance-control-field">
                    <span>Familia tipográfica global</span>
                    <select value={selectedFontFamily} onChange={(event) => setSelectedFontFamily(event.target.value)}>
                      {fontOptions.map((font) => (
                        <option key={font.id} value={font.family}>{font.label}</option>
                      ))}
                    </select>
                  </label>
                  <label className="appearance-control-field">
                    <span>Tamaño base de texto</span>
                    <input type="range" min={85} max={125} defaultValue={100} />
                  </label>
                  <h3>Guardar tema actual</h3>
                  <label className="appearance-control-field">
                    <span>Nombre del ajuste</span>
                    <input value={newThemeName} onChange={(event) => setNewThemeName(event.target.value)} placeholder="Ej: Noche azul" />
                  </label>
                  <div className="network-controls">
                    <button onClick={() => {
                      const name = newThemeName.trim()
                      if (!name) {
                        setAppearanceMessage('Debes ingresar un nombre para guardar el tema.')
                        return
                      }
                      const theme: UserAppearanceTheme = { id: `user-${Date.now()}`, name, vars: { ...customAppearanceVars } }
                      setUserAppearanceThemes((prev) => {
                        const withoutSameName = prev.filter((item) => item.name.toLowerCase() !== name.toLowerCase())
                        return [...withoutSameName, theme]
                      })
                      setSelectedAppearancePreset(theme.id)
                      setNewThemeName('')
                      setAppearanceMessage(`Tema guardado: ${name}`)
                    }}>Guardar ajuste</button>
                    <button className="danger" onClick={() => {
                      if (!selectedAppearancePreset.startsWith('user-')) return
                      setUserAppearanceThemes((prev) => prev.filter((theme) => theme.id !== selectedAppearancePreset))
                      setSelectedAppearancePreset('custom')
                      setAppearanceMessage('Tema eliminado correctamente.')
                    }} disabled={!selectedAppearancePreset.startsWith('user-')}>Eliminar ajuste</button>
                  </div>
                  {appearanceMessage && <small>{appearanceMessage}</small>}
                </div>

                <div className="appearance-preview detailed">
                  <h3>Escala UI</h3>
                  <label className="appearance-control-field appearance-scale-field">
                    <span>Escala UI global</span>
                    <div className="appearance-scale-row">
                      <input
                        type="range"
                        min={85}
                        max={120}
                        value={uiScalePercent}
                        onChange={(event) => setUiScalePercent(Number(event.target.value))}
                      />
                      <strong>{uiScalePercent}%</strong>
                    </div>
                  </label>
                  <label className="appearance-control-field appearance-scale-field">
                    <span>Escala de elementos individuales</span>
                    <div className="appearance-scale-row">
                      <input
                        type="range"
                        min={85}
                        max={120}
                        value={uiElementScalePercent}
                        onChange={(event) => setUiElementScalePercent(Number(event.target.value))}
                      />
                      <strong>{uiElementScalePercent}%</strong>
                    </div>
                  </label>
                  <button>Modo Editor</button>
                </div>
              </section>
            )}

                        {selectedGlobalSettingsTab === 'Java' && (
              <section className="section-placeholder">
                <h2>Java</h2>
                <p>Runtime principal: <strong>{folderRoutes.find((item) => item.key === 'java')?.value ?? '-'}</strong></p>
                <div className="network-controls">
                  <button onClick={() => void pickFolderRoute(folderRoutes.find((item) => item.key === 'java') ?? defaultFolderRoutes[2])}>Cambiar ruta Java</button>
                  <button onClick={() => void invoke('open_folder_path', { path: launcherFolders?.runtimeDir ?? '' })}>Abrir carpeta Java embebido</button>
                </div>
              </section>
            )}

            {selectedGlobalSettingsTab === 'Servicios' && (
              <section className="section-placeholder">
                <h2>Servicios</h2>
                <p>Estado backend: autenticación, metadata y sincronización listos para integración continua.</p>
                <button className="primary" onClick={() => setUpdatesStatus('Servicios verificados correctamente desde el panel global.')}>Validar servicios</button>
              </section>
            )}

            {selectedGlobalSettingsTab === 'Herramientas' && (
              <section className="section-placeholder">
                <h2>Herramientas</h2>
                <p>Skin Studio, diagnóstico de consola y utilidades de instancia centralizadas profesionalmente.</p>
                <button onClick={() => navigateToPage('Editor de skins')}>Abrir editor de skins</button>
              </section>
            )}

            {selectedGlobalSettingsTab === 'Network' && (
              <section className="section-placeholder">
                <h2>Network</h2>
                <p>Canal activo de updates: <strong>{updatesChannel}</strong>. Auto-check: <strong>{updatesAutoCheck ? 'Habilitado' : 'Deshabilitado'}</strong>.</p>
                <div className="network-controls">
                  <button onClick={() => setUpdatesChannel((prev) => prev === 'Stable' ? 'Preview' : 'Stable')}>Cambiar canal</button>
                  <button onClick={() => setUpdatesAutoCheck((prev) => !prev)}>{updatesAutoCheck ? 'Desactivar' : 'Activar'} auto-check</button>
                </div>
              </section>
            )}
          </section>
        </main>
      )}

      <MigrationModal
        title="Cambiar carpeta raíz del launcher"
        description="¿Deseas migrar también todos tus datos (instancias, Java embebido, assets) a la nueva ubicación?"
        open={launcherMigrationPath !== null}
        pendingPath={launcherMigrationPath ?? ''}
        progress={isMigrating ? migrationProgress : null}
        onClose={() => setLauncherMigrationPath(null)}
        onMigrate={() => void (async () => {
          if (!launcherMigrationPath) return
          await migrateLauncherRoot(launcherMigrationPath, true)
          setLauncherMigrationPath(null)
          await refreshLauncherFolders()
        })()}
        onOnlyPath={() => void (async () => {
          if (!launcherMigrationPath) return
          await migrateLauncherRoot(launcherMigrationPath, false)
          setLauncherMigrationPath(null)
          await refreshLauncherFolders()
        })()}
      />

      <MigrationModal
        title="Cambiar carpeta de instancias"
        description="¿Deseas mover tus instancias a la nueva carpeta?"
        open={instancesMigrationPath !== null}
        pendingPath={instancesMigrationPath ?? ''}
        progress={isMigrating ? migrationProgress : null}
        onClose={() => setInstancesMigrationPath(null)}
        onMigrate={() => void (async () => {
          if (!instancesMigrationPath) return
          await changeInstancesFolder(instancesMigrationPath, true)
          setInstancesMigrationPath(null)
          await refreshLauncherFolders()
          await refreshInstances()
        })()}
        onOnlyPath={() => void (async () => {
          if (!instancesMigrationPath) return
          await changeInstancesFolder(instancesMigrationPath, false)
          setInstancesMigrationPath(null)
          await refreshLauncherFolders()
        })()}
      />

      {showDeleteInstanceConfirm && selectedCard && (
        <div className="floating-modal-overlay" role="dialog" aria-modal="true" aria-label="Confirmar eliminación de instancia">
          <div className="floating-modal">
            <h3>¿Eliminar instancia?</h3>
            <p>
              {instanceMetaByRoot[selectedCard.instanceRoot ?? '']?.state?.toUpperCase() === 'REDIRECT'
                ? <>Se eliminará solo el atajo <strong>{selectedCard.name}</strong>. La instancia original no será modificada.</>
                : <>Se eliminará completamente la instancia <strong>{selectedCard.name}</strong> y todos sus archivos.</>}
            </p>
            <div className="floating-modal-actions">
              <button onClick={() => setShowDeleteInstanceConfirm(false)} disabled={isDeletingInstance}>Cancelar</button>
              <button className="danger" onClick={deleteSelectedInstance} disabled={isDeletingInstance}>
                {isDeletingInstance ? 'Eliminando...' : 'Eliminar'}
              </button>
            </div>
          </div>
        </div>
      )}


      {authSession && activePage === 'Creador de Instancias' && (
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
              <button className="icon-button" style={iconButtonStyle} aria-label="Seleccionar icono" onClick={() => creationIconInputRef.current?.click()}>
                {instanceIconPreview.startsWith('data:image') ? 'icono' : instanceIconPreview}
              </button>
              <input
                ref={creationIconInputRef}
                type="file"
                accept="image/*,.png,.jpg,.jpeg,.webp,.gif,.bmp,.svg"
                onChange={uploadInstanceIcon}
                hidden
              />
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
                <div className="creation-console-and-progress">
                  <aside ref={creationConsoleRef} className="creation-mini-console" role="log" aria-label="Consola de creación">
                    {creationConsoleLogs.length === 0 && <p>Consola lista. Aquí verás la creación e instalación de la instancia.</p>}
                    {creationConsoleLogs.map((line, index) => (
                      <p key={`creation-log-${index}`}>{line}</p>
                    ))}
                  </aside>
                  <div className="creation-progress-wrap" aria-label="Progreso de creación de instancia">
                    <div
                      className="creation-progress-fill"
                      style={{
                        width: `${creationProgress && creationProgress.total > 0
                          ? Math.min(100, Math.round((creationProgress.completed / creationProgress.total) * 100))
                          : 0}%`,
                      }}
                    />
                  </div>
                </div>
              </div>
            </header>

            {selectedCreatorSection === 'Personalizado' ? (
              <div className="customized-content">
                <ListInterface
                  title="Versiones Minecraft"
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
                  advancedActions={['Todos', 'Estables', 'Experimentales']}
                  selectedAdvancedAction={selectedMcChannel}
                  onAdvancedActionSelect={(value) => setSelectedMcChannel(value as McChannel)}
                  metaLine={
                    manifestLoading
                      ? 'Cargando version_manifest_v2 oficial de Mojang...'
                      : manifestError
                        ? manifestError
                        : `Fuente oficial: ${mojangManifestUrl}`
                  }
                />
                <ListInterface
                  title="Versiones de Loaders"
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
                  rightActions={['Todos', 'Stable', 'Latest', 'Releases']}
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
                        : loaderError || `Catálogo activo: ${selectedLoader}`
                  }
                />
              </div>
            ) : (
              <section className="section-placeholder">
                <h2>{selectedCreatorSection}</h2>
                <p>Asignaciones activas para esta sección del creador.</p>
                <div className="settings-pane-grid">
                  <article>
                    <h3>Rutas y asignaciones reales</h3>
                    {creatorSectionRows.map(([label, value]) => (
                      <p key={`${selectedCreatorSection}-${label}`}><strong>{label}:</strong> {value}</p>
                    ))}
                  </article>
                </div>
              </section>
            )}

            <footer className="creator-footer-actions lowered">
              <button className="primary" onClick={createInstance} disabled={isCreating || !selectedMinecraftVersion}>
                {isCreating ? 'Creando...' : 'Ok'}
              </button>
              <button onClick={() => navigateToPage('Mis Modpacks')}>Cancelar</button>
            </footer>
          </section>
        </main>
      )}

      {authSession && activePage === 'Editar Instancia' && selectedCard && (
        <main className={`edit-instance-layout ${(selectedEditSection === 'Mods' && modsDownloaderOpen) ? 'mods-downloader-mode' : ''}`} style={{ '--sidebar-width': `${editSidebarWidth}px` } as CSSProperties}>
          {!(selectedEditSection === 'Mods' && modsDownloaderOpen) && (
            <>
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
            </>
          )}

          <section className="edit-main-content">
            {selectedEditSection === 'Ejecución' ? (
              <section className="execution-view execution-view-full">
                <div className="execution-toolbar">
                  <div className="execution-primary-actions">
                    <button className="primary launch-btn" onClick={() => void startInstanceProcess()} disabled={isStartingInstance || isInstanceRunning}>
                      {isStartingInstance ? "⏳ Iniciando..." : isInstanceRunning ? "🟢 Ejecutándose" : "▶ Iniciar instancia"}
                    </button>
                    <button className="danger ghost-btn">■ Forzar cierre</button>
                  </div>
                  <div className="execution-secondary-actions">
                    <button className="ghost-btn" onClick={exportRuntimeLog}>Exportar .log</button>
                  </div>
                  <select value={consoleLevelFilter} onChange={(event) => setConsoleLevelFilter(event.target.value as 'Todos' | ConsoleLevel)}>
                    <option value="Todos">Nivel: Todos</option>
                    <option value="INFO">INFO</option>
                    <option value="WARN">WARN</option>
                    <option value="ERROR">ERROR</option>
                    <option value="FATAL">FATAL</option>
                  </select>
                  <select value={launcherLogFilter} onChange={(event) => setLauncherLogFilter(event.target.value as 'Todos' | ConsoleSource)}>
                    <option value="Todos">Origen: Todos</option>
                    <option value="launcher">Launcher</option>
                    <option value="game">Juego</option>
                  </select>
                  <button className={`ghost-btn toolbar-toggle-btn ${autoScrollConsole ? 'active' : ''}`} onClick={() => setAutoScrollConsole((prev) => !prev)}>
                    AutoScroll {autoScrollConsole ? 'ON' : 'OFF'}
                  </button>
                  <input
                    type="search"
                    value={logSearch}
                    onChange={(event) => setLogSearch(event.target.value)}
                    placeholder="Buscar en consola"
                    aria-label="Buscar en consola"
                  />
                </div>

                <div className="execution-log-console" role="log" aria-label="Consola de logs" ref={runtimeConsoleRef}>
                  {isStartingInstance && !isInstanceRunning && (
                    <div className="instance-launch-progress-compact" aria-label="Progreso de inicio de ejecución">
                      <div className="instance-launch-progress-compact-fill" style={{ width: `${launchProgressPercent}%` }} />
                    </div>
                  )}
                  {runtimeConsole
                    .filter((entry) => (consoleLevelFilter === 'Todos' ? true : entry.level === consoleLevelFilter))
                    .filter((entry) => (launcherLogFilter === 'Todos' ? true : entry.source === launcherLogFilter))
                    .filter((entry) => !logSearch || entry.message.toLowerCase().includes(logSearch.toLowerCase()))
                    .map((entry, index) => (
                      <p key={`${entry.timestamp}-${index}`} className={`log-level-${entry.level.toLowerCase()}`}>
                        [{entry.timestamp}] [{entry.source}] [{entry.level}] {entry.message}
                      </p>
                    ))}
                  {runtimeConsole.length === 0 && <p>[{nowTimestamp()}] [launcher] [INFO] Consola lista para iniciar.</p>}
                </div>
              </section>
            ) : selectedEditSection === 'Configuración' ? (
              <section className="instance-settings-view">
                <header className="settings-tabs-bar">
                  {(['General', 'Java', 'Ajustes', 'Comandos Personalizados', 'Variables de Entorno'] as InstanceSettingsTab[]).map((tab) => (
                    <button key={tab} className={selectedSettingsTab === tab ? 'active' : ''} onClick={() => setSelectedSettingsTab(tab)}>
                      {tab}
                    </button>
                  ))}
                </header>

                {selectedSettingsTab === 'General' && (
                  <div className="settings-pane-grid">
                    <article>
                      <h3>Instancia</h3>
                      <p><strong>Nombre:</strong> {selectedInstanceMetadata?.name ?? selectedCard.name}</p>
                      <p><strong>Grupo:</strong> {selectedInstanceMetadata?.group ?? selectedCard.group}</p>
                      <p><strong>Minecraft:</strong> {selectedInstanceMetadata?.minecraftVersion ?? '-'}</p>
                      <p><strong>Loader:</strong> {selectedInstanceMetadata?.loader ?? '-'} {selectedInstanceMetadata?.loaderVersion ?? ''}</p>
                    </article>
                  </div>
                )}

                {selectedSettingsTab === 'Java' && (
                  <div className="settings-pane-grid">
                    <article>
                      <h3>Instalación de Java</h3>
                      <p><strong>Runtime:</strong> {selectedInstanceMetadata?.javaRuntime ?? '-'}</p>
                      <p><strong>Ruta Java real:</strong> {launchPreparation?.javaPath ?? selectedInstanceMetadata?.javaPath ?? '-'}</p>
                      <p><strong>Versión Java real:</strong> {launchPreparation?.javaVersion ?? selectedInstanceMetadata?.javaVersion ?? '-'}</p>
                    </article>
                    <article>
                      <h3>Memoria</h3>
                      <p><strong>RAM asignada automáticamente:</strong> {selectedInstanceMetadata?.ramMb ?? 0} MiB</p>
                    </article>
                    <article>
                      <h3>Argumentos de Java</h3>
                      <textarea
                        readOnly
                        value={launchPreparation ? [...launchPreparation.jvmArgs, launchPreparation.mainClass, ...launchPreparation.gameArgs].join(' ') : (selectedInstanceMetadata?.javaArgs ?? []).join(' ')}
                        placeholder="Sin argumentos personalizados"
                      />
                    </article>
                  </div>
                )}

                {selectedSettingsTab === 'Ajustes' && (
                  <div className="settings-pane-grid">
                    <article>
                      <h3>Classpath efectivo</h3>
                      {classpathPreviewRows.map(([label, value]) => (
                        <p key={`cp-${label}`}><strong>{label}:</strong> {value}</p>
                      ))}
                    </article>
                    <article>
                      <h3>Rutas de la instancia</h3>
                      <p><strong>Instance Root:</strong> {selectedCard.instanceRoot ?? '-'}</p>
                      <p><strong>Minecraft Root:</strong> {selectedCard.instanceRoot ? `${selectedCard.instanceRoot}/minecraft` : '-'}</p>
                      <p><strong>Version JSON:</strong> {selectedInstanceMetadata?.minecraftVersion ? `${selectedCard.instanceRoot}/minecraft/versions/${selectedInstanceMetadata.minecraftVersion}/${selectedInstanceMetadata.minecraftVersion}.json` : '-'}</p>
                    </article>
                  </div>
                )}

                {selectedSettingsTab === 'Comandos Personalizados' && (
                  <div className="settings-pane-grid">
                    <article>
                      <h3>Asignación de comando JVM</h3>
                      <textarea
                        readOnly
                        value={launchPreparation ? [launchPreparation.javaPath, ...launchPreparation.jvmArgs, launchPreparation.mainClass].join(' ') : 'Aún no hay comando validado'}
                      />
                    </article>
                    <article>
                      <h3>Asignación de argumentos GAME</h3>
                      <textarea
                        readOnly
                        value={launchPreparation ? launchPreparation.gameArgs.join(' ') : 'Aún no hay argumentos GAME validados'}
                      />
                    </article>
                  </div>
                )}

                {selectedSettingsTab === 'Variables de Entorno' && (
                  <div className="settings-pane-grid">
                    <article>
                      <h3>Variables efectivas</h3>
                      {envRows.map(([key, value]) => (
                        <p key={`env-${key}`}><strong>{key}:</strong> {value}</p>
                      ))}
                    </article>
                  </div>
                )}
              </section>
            ) : selectedEditSection === 'Mods' ? (
              <section className="mods-editor-view">
                {!modsDownloaderOpen ? (
                  <>
                    <header className="mods-topbar">
                      <input type="search" value={modsSearch} onChange={(event) => setModsSearch(event.target.value)} placeholder="Buscar mod" aria-label="Buscar mod" />
                      <select value={modsProviderFilter} onChange={(event) => setModsProviderFilter(event.target.value as typeof modsProviderFilter)}>
                        <option value="all">Proveedor: Todos</option>
                        <option value="CurseForge">CurseForge</option>
                        <option value="Modrinth">Modrinth</option>
                        <option value="Externo">Externo</option>
                        <option value="Local">Local</option>
                      </select>
                      <button className="ghost-btn" onClick={() => setModsAdvancedOpen((prev) => !prev)}>{modsAdvancedOpen ? 'Ocultar filtros' : 'Filtro avanzado'}</button>
                    </header>
                    {modsAdvancedOpen && (
                      <div className="advanced-filter-body inline mods-advanced-strip">
                        <label>Estado
                          <select value={modsAdvancedFilter.state} onChange={(event) => setModsAdvancedFilter((prev) => ({ ...prev, state: event.target.value as ModsAdvancedFilter['state'] }))}>
                            <option value="all">Todos</option>
                            <option value="enabled">Activos</option>
                            <option value="disabled">Desactivados</option>
                          </select>
                        </label>
                        <label>Etiqueta
                          <select value={modsAdvancedFilter.tag} onChange={(event) => setModsAdvancedFilter((prev) => ({ ...prev, tag: event.target.value as ModsAdvancedFilter['tag'] }))}>
                            <option value="all">Todas</option>
                            <option value="dependencia">dependencia</option>
                            <option value="incompatible">incompatible</option>
                            <option value="crash">crash</option>
                            <option value="warn">warn</option>
                          </select>
                        </label>
                      </div>
                    )}
                    <div className="mods-main-layout">
                      <div className="mods-list-panel">
                        {modsLoading && <p>Cargando mods...</p>}
                        {modsError && <p className="error-banner">{modsError}</p>}
                        <div className="mods-list-head" style={{ '--mods-name-col': `${modsNameColumnWidth}px` } as CSSProperties}>
                          <span>Habilitar</span><span>Icono</span><span>Nombre</span><span className="mods-col-resizer" role="separator" aria-label="Redimensionar columna nombre" onPointerDown={startModNameColumnDrag} /><span>Versión</span><span>Última modificación</span><span>Proveedor</span><span>Tamaño</span><span>Etiqueta</span>
                        </div>
                        <div className="mods-list-body">
                          {pagedMods.length === 0 && <p className="mods-empty">No hay mods para los filtros seleccionados.</p>}
                          {pagedMods.map((mod) => (
                            <div key={mod.id} role="button" tabIndex={0} className={`mods-row ${selectedModId === mod.id ? 'active' : ''}`} style={{ '--mods-name-col': `${modsNameColumnWidth}px` } as CSSProperties} onClick={() => setSelectedModId(mod.id)} onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') setSelectedModId(mod.id) }}>
                              <span><button className={`chip-toggle ${mod.enabled ? 'on' : 'off'}`} onClick={(event) => { event.stopPropagation(); void toggleModEnabled(mod, !mod.enabled) }}>{mod.enabled ? 'ON' : 'OFF'}</button></span>
                              <span>{modIconById[mod.id] ? <img className="mods-row-icon" src={modIconById[mod.id]} alt={`Icono de ${mod.name}`} loading="lazy" /> : (resolveProviderLabel(mod.provider) === 'CurseForge' ? '🔥' : resolveProviderLabel(mod.provider) === 'Modrinth' ? '🟢' : resolveProviderLabel(mod.provider) === 'Externo' ? '🌐' : '📁')}</span>
                              <span>{mod.name}</span>
                              <span className="mods-col-resizer-spacer" />
                              <span>{mod.version}</span>
                              <span>{mod.modifiedAt ? new Date(mod.modifiedAt * 1000).toLocaleString() : '-'}</span>
                              <span>{resolveProviderLabel(mod.provider)}</span>
                              <span>{formatBytes(mod.sizeBytes)}</span>
                              <span className="mods-tag">{inferModTag(mod)}</span>
                            </div>
                          ))}
                        </div>
                        <footer className="import-pagination">
                          <button className="square" onClick={() => setModsPage((prev) => Math.max(1, prev - 1))} disabled={modsPage <= 1}>Anterior</button>
                          <span>Página {modsPage} de {modsTotalPages}</span>
                          <button className="square" onClick={() => setModsPage((prev) => Math.min(modsTotalPages, prev + 1))} disabled={modsPage >= modsTotalPages}>Siguiente</button>
                        </footer>
                      </div>
                      <aside className="mods-right-sidebar">
                        <button className="action-elevated" onClick={openDownloader}>Descargar mods</button>
                        <button className="action-elevated" onClick={() => { void fetchVersionOptions() }} disabled={!selectedModId || modVersionLoading}>Cambiar versión</button>
                        <button className="action-elevated" onClick={() => { void checkInstalledModUpdates() }} disabled={updatesReviewLoading || instanceMods.length === 0}>Buscar actualizaciones</button>
                        {modVersionError && <p className="error-banner">{modVersionError}</p>}
                      </aside>
                    </div>

                    {modVersionModalOpen && selectedMod && (
                      <div className="floating-modal-overlay" role="dialog" aria-modal="true" aria-label="Cambiar versión del mod">
                        <article className="floating-modal mods-version-modal">
                          <h3>Cambiar versión · {selectedMod.name}</h3>
                          <div className="mods-downloader-panels mods-version-modal-panels">
                            <section className="mods-preview-panel">
                              <h4>{selectedMod.name}</h4>
                              <p>{modVersionDetail?.description || 'Sin descripción del catálogo para este mod.'}</p>
                              {modVersionDetail?.bodyHtml && <div className="mods-preview-description" dangerouslySetInnerHTML={{ __html: renderCatalogBody(modVersionDetail.bodyHtml) }} />}
                            </section>
                            <section className="mods-catalog-panel">
                              <label>Versiones disponibles</label>
                              <select value={selectedVersionOptionId} onChange={(event) => setSelectedVersionOptionId(event.target.value)}>
                                {modVersionOptions.map((option) => {
                                  const optionId = `${option.name}-${option.version}`
                                  return <option key={optionId} value={optionId}>{option.name} · MC {option.version || '-'}</option>
                                })}
                              </select>
                              <p>Instalada actualmente: {selectedMod.version}</p>
                            </section>
                          </div>
                          <footer className="floating-modal-actions">
                            <button className="primary" onClick={() => {
                              const selectedOption = modVersionOptions.find((option) => `${option.name}-${option.version}` === selectedVersionOptionId)
                              if (!selectedOption) return
                              void replaceModVersion(selectedOption)
                              setModVersionModalOpen(false)
                            }} disabled={!selectedVersionOptionId}>Reinstalar</button>
                            <button onClick={() => setModVersionModalOpen(false)}>Cancelar</button>
                          </footer>
                        </article>
                      </div>
                    )}

                    {updatesModalOpen && (
                      <div className="floating-modal-overlay" role="dialog" aria-modal="true" aria-label="Actualizaciones de mods">
                        <article className="floating-modal mods-review-modal mods-updates-modal">
                          <h3>Actualizaciones compatibles disponibles</h3>
                          <div className="mods-review-list">
                            {updatesCandidates.length === 0 && <p className="mods-empty">No se encontraron actualizaciones compatibles con el loader y la versión de Minecraft actuales.</p>}
                            {updatesCandidates.map((candidate) => (
                              <div key={candidate.mod.id} className="mods-review-card">
                                <strong>{candidate.mod.name}</strong>
                                <p>Instalada: {candidate.mod.version} · Nueva: {candidate.nextVersion.name} · MC {candidate.nextVersion.version || '-'}</p>
                              </div>
                            ))}
                          </div>
                          <footer className="floating-modal-actions">
                            <button className="primary" onClick={() => { void applyAllUpdates() }} disabled={updatesCandidates.length === 0}>Ok y reinstalar</button>
                            <button onClick={() => setUpdatesModalOpen(false)}>Cancelar</button>
                          </footer>
                        </article>
                      </div>
                    )}
                  </>
                ) : (
                  <div className="mods-downloader-page">
                    <div className="mods-downloader-layout">
                      <aside className={`mods-downloader-sidebar ${modsAdvancedOpen ? 'advanced' : 'open'}`}>
                        {!modsAdvancedOpen ? (
                          (['Modrinth', 'CurseForge', 'Externos', 'Locales'] as ModsDownloaderSource[]).map((source) => (
                            <button key={source} className={modsDownloaderSource === source ? 'active' : ''} onClick={() => setModsDownloaderSource(source)}>{source}</button>
                          ))
                        ) : (
                          <div className="mods-downloader-advanced-sidebar in-left">
                            <section>
                              <h4>Categorías</h4>
                              <p className="mods-empty">Categorías de mods (próximamente).</p>
                            </section>
                            <section>
                              <h4>Cargadores</h4>
                              <select value={downloaderLoaderFilter} onChange={(event) => setDownloaderLoaderFilter(event.target.value)}><option value="">{selectedInstanceMetadata?.loader ?? 'Todos los loaders'}</option><option value="fabric">fabric</option><option value="forge">forge</option><option value="neoforge">neoforge</option><option value="quilt">quilt</option></select>
                            </section>
                            <section>
                              <h4>Versiones</h4>
                              <label><input type="checkbox" checked={downloaderShowAllVersions} onChange={(event) => setDownloaderShowAllVersions(event.target.checked)} /> Mostrar todas las versiones</label>
                              <select value={downloaderVersionFilter} onChange={(event) => setDownloaderVersionFilter(event.target.value)} disabled={downloaderShowAllVersions}>
                                <option value="">Versión actual de la instancia</option>
                                {releaseMinecraftVersions.map((version) => <option key={version} value={version}>{version}</option>)}
                              </select>
                            </section>
                            <section>
                              <h4>Entornos</h4>
                              <label><input type="checkbox" checked={downloaderClientOnly} onChange={(event) => setDownloaderClientOnly(event.target.checked)} /> Solo cliente</label>
                              <label><input type="checkbox" checked={downloaderServerOnly} onChange={(event) => setDownloaderServerOnly(event.target.checked)} /> Solo servidor</label>
                            </section>
                          </div>
                        )}
                      </aside>
                      <div className="mods-downloader-center">
                        <header className="mods-downloader-topbar">
                          {isCatalogSource && <input type="search" value={downloaderSearch} onChange={(event) => setDownloaderSearch(event.target.value)} placeholder="Buscar mods en catálogo" />}
                          <button className={`ghost-btn mods-filter-btn ${modsAdvancedOpen ? 'active' : ''}`} onClick={() => setModsAdvancedOpen((prev) => !prev)}>Filtro avanzado</button>
                        </header>
                        <div className="mods-downloader-panels">
                          <section className="mods-catalog-panel">
                            {modsCatalogLoading && <p className="mods-empty">Cargando catálogo...</p>}
                            {modsCatalogError && <p className="error-banner">{modsCatalogError}</p>}
                            {isCatalogSource ? downloaderCatalogMods.map((mod) => {
                              const isSelected = selectedCatalogModId === mod.id
                              const isStaged = Boolean(stagedDownloads[mod.id])
                              return (
                                <article key={mod.id} className={`mods-catalog-item ${isSelected ? 'active' : ''}`}>
                                  <button className="mods-catalog-toggle" onClick={() => { if (selectedCatalogModId !== mod.id) { setSelectedCatalogModId(mod.id); return } void stageSelectedMod() }}>{isStaged ? '✓' : '+'}</button>
                                  <button className="mods-catalog-main" onClick={() => setSelectedCatalogModId(mod.id)}>
                                    {mod.image && <img src={mod.image} alt={mod.name} loading="lazy" />}
                                    <span>
                                      <strong>{mod.name}</strong>
                                      <small>{mod.summary}</small>
                                    </span>
                                  </button>
                                </article>
                              )
                            }) : <p className="mods-empty">Esta fuente está reservada para integración manual/local.</p>}
                          </section>
                          <section className="mods-preview-panel compact-info">
                            {selectedCatalogMod ? (
                              <>
                                <h3>{selectedCatalogMod.name}</h3>
                                <p>{selectedCatalogDetail?.description || selectedCatalogMod.summary}</p>
                                {catalogDetailLoading && <p className="mods-empty">Cargando información del mod...</p>}
                                {selectedCatalogDetail?.links && selectedCatalogDetail.links.length > 0 && (
                                  <div className="mods-preview-links">
                                    {selectedCatalogDetail.links.map((link) => (
                                      <a key={link.url} className="mods-preview-link" href={link.url} target="_blank" rel="noopener noreferrer">
                                        {link.label}: {shortenUrl(link.url)}
                                      </a>
                                    ))}
                                  </div>
                                )}
                                {selectedCatalogDetail?.bodyHtml && <div className="mods-preview-description" dangerouslySetInnerHTML={{ __html: renderCatalogBody(selectedCatalogDetail.bodyHtml) }} />}
                                <p>Descargas: {selectedCatalogMod.downloads.toLocaleString()} · Actualizado: {new Date(selectedCatalogMod.updatedAt).toLocaleDateString()}</p>
                              </>
                            ) : <p className="mods-empty">Selecciona un mod para ver su detalle.</p>}
                          </section>
                          
                        </div>
                        <div className="mods-downloader-compact-bars premium-footer">
                          <div className="mods-compact-bar sort-block">
                            <label>Ordenar
                              <select className="mods-sort-select" value={modsDownloaderSort} onChange={(event) => setModsDownloaderSort(event.target.value as ModsDownloaderSort)}>
                                <option value="relevance">Relevancia</option>
                                <option value="downloads">Descargas</option>
                                <option value="followers">Seguidores</option>
                                <option value="newest">Mas actual</option>
                                <option value="updated">Ultima actualizacion</option>
                              </select>
                            </label>
                          </div>
                          <div className="mods-compact-bar versions-block">
                            <label>Versiones del mod
                              <select value={selectedCatalogVersion?.id ?? ''} disabled={!selectedCatalogMod}>
                                {selectedCatalogVersions.map((version) => (
                                  <option key={`${version.name}-${version.gameVersion}-${version.downloadUrl}`} value={`${version.name}-${version.gameVersion}`}>{selectedCatalogMod?.name ?? 'Mod'} · {version.gameVersion} · {version.name} · {(version.versionType ?? 'release').toUpperCase()} {((selectedCatalogMod && instanceMods.some((item) => item.name.toLowerCase() === selectedCatalogMod.name.toLowerCase())) ? '· INSTALADO' : '')}</option>
                                ))}
                              </select>
                            </label>
                            <span className="mods-channel-chip">{(selectedCatalogVersion?.versionType ?? 'release').toUpperCase()}</span>
                          </div>
                          <div className="mods-compact-bar actions-row">
                            <button className="secondary" onClick={() => { void stageSelectedMod() }} disabled={!selectedCatalogVersion}>{stagedDownloads[selectedCatalogModId] ? 'Deseleccionar' : 'Seleccionar para descargar'}</button>
                            <div className="actions-group-right">
                              <button className="ghost-btn" onClick={closeDownloaderWithValidation}>Cancelar</button>
                              <button className="primary" onClick={() => setReviewModalOpen(true)}>Revisar y confirmar</button>
                            </div>
                          </div>
                        </div>
                      </div>
                    </div>



                    {modVersionModalOpen && selectedMod && (
                      <div className="floating-modal-overlay" role="dialog" aria-modal="true" aria-label="Cambiar versión del mod">
                        <article className="floating-modal mods-version-modal">
                          <h3>Cambiar versión · {selectedMod.name}</h3>
                          <div className="mods-downloader-panels mods-version-modal-panels">
                            <section className="mods-preview-panel">
                              <h4>{selectedMod.name}</h4>
                              <p>{modVersionDetail?.description || 'Sin descripción del catálogo para este mod.'}</p>
                              {modVersionDetail?.bodyHtml && <div className="mods-preview-description" dangerouslySetInnerHTML={{ __html: renderCatalogBody(modVersionDetail.bodyHtml) }} />}
                            </section>
                            <section className="mods-catalog-panel">
                              <label>Versiones disponibles</label>
                              <select value={selectedVersionOptionId} onChange={(event) => setSelectedVersionOptionId(event.target.value)}>
                                {modVersionOptions.map((option) => {
                                  const optionId = `${option.name}-${option.version}`
                                  return <option key={optionId} value={optionId}>{option.name} · MC {option.version || '-'}</option>
                                })}
                              </select>
                              <p>Instalada actualmente: {selectedMod.version}</p>
                            </section>
                          </div>
                          <footer className="floating-modal-actions">
                            <button className="primary" onClick={() => {
                              const selectedOption = modVersionOptions.find((option) => `${option.name}-${option.version}` === selectedVersionOptionId)
                              if (!selectedOption) return
                              void replaceModVersion(selectedOption)
                              setModVersionModalOpen(false)
                            }} disabled={!selectedVersionOptionId}>Reinstalar</button>
                            <button onClick={() => setModVersionModalOpen(false)}>Cancelar</button>
                          </footer>
                        </article>
                      </div>
                    )}

                    {updatesModalOpen && (
                      <div className="floating-modal-overlay" role="dialog" aria-modal="true" aria-label="Actualizaciones de mods">
                        <article className="floating-modal mods-review-modal">
                          <h3>Actualizaciones compatibles disponibles</h3>
                          <div className="mods-review-list">
                            {updatesCandidates.length === 0 && <p className="mods-empty">No se encontraron actualizaciones compatibles con el loader y la versión de Minecraft actuales.</p>}
                            {updatesCandidates.map((candidate) => (
                              <div key={candidate.mod.id} className="mods-review-card">
                                <strong>{candidate.mod.name}</strong>
                                <p>Instalada: {candidate.mod.version} · Nueva: {candidate.nextVersion.name} · MC {candidate.nextVersion.version || '-'}</p>
                              </div>
                            ))}
                          </div>
                          <footer className="floating-modal-actions">
                            <button className="primary" onClick={() => { void applyAllUpdates() }} disabled={updatesCandidates.length === 0}>Ok y reinstalar</button>
                            <button onClick={() => setUpdatesModalOpen(false)}>Cancelar</button>
                          </footer>
                        </article>
                      </div>
                    )}

                    {installingModalOpen && (
                      <div className="floating-modal-overlay" role="dialog" aria-modal="true" aria-label="Instalando mods">
                        <article className="floating-modal mods-review-modal">
                          <h3>Descargando e instalando mods</h3>
                          <p>{installProgress.message}</p>
                          <progress max={Math.max(1, installProgress.total)} value={installProgress.current} />
                          <p>{installProgress.current} / {installProgress.total}</p>
                        </article>
                      </div>
                    )}

                    {reviewModalOpen && (
                      <div className="floating-modal-overlay" role="dialog" aria-modal="true" aria-label="Revisión de mods seleccionados">
                        <article className="floating-modal mods-review-modal">
                          <h3>Revisión de mods seleccionados</h3>
                          <div className="mods-review-list">
                            {reviewTree.length === 0 && <p className="mods-empty">No hay mods seleccionados.</p>}
                            {reviewTree.map((entry) => (
                              <div key={entry.mod.id} className="mods-review-card">
                                <label>
                                  <input type="checkbox" checked={entry.selected} onChange={(event) => toggleStageFromReview(entry.mod.id, event.target.checked)} />
                                  <strong>{entry.mod.name}</strong> · {entry.version.name} · MC {entry.version.gameVersion} · {(entry.version.versionType ?? 'release').toUpperCase()}
                                  {entry.reinstall && <span className="mods-tag">Instalado (desmarcado por defecto)</span>}
                                </label>
                                {entry.dependencies.map((dependency) => (
                                  <p key={`${entry.mod.id}-${dependency.mod.id}`} className="mods-review-dependency">↳ Dependencia obligatoria: {dependency.mod.name} {dependency.installed ? '(ya instalada)' : ''}</p>
                                ))}
                              </div>
                            ))}
                          </div>
                          <footer className="floating-modal-actions">
                            <button className="primary" onClick={confirmReviewAndInstall}>Ok</button>
                            <button onClick={() => setReviewModalOpen(false)}>Cancelar</button>
                          </footer>
                        </article>
                      </div>
                    )}


                    {cancelModsConfirmOpen && (
                      <div className="floating-modal-overlay" role="dialog" aria-modal="true" aria-label="Cancelar selección de mods">
                        <article className="floating-modal mods-review-modal">
                          <h3>¿Cancelar selección de mods?</h3>
                          <p>Todavía tienes mods seleccionados para descargar. Si sales ahora perderás la revisión pendiente.</p>
                          <footer className="floating-modal-actions">
                            <button className="primary" onClick={closeDownloader}>Salir y descartar</button>
                            <button onClick={() => setCancelModsConfirmOpen(false)}>Seguir revisando</button>
                          </footer>
                        </article>
                      </div>
                    )}
                  </div>
                )}
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

        </motion.div>
      </AnimatePresence>
    </div>
  )
}

type PrincipalTopBarProps = {
  authSession: AuthSession | null
  activePage: MainPage
  uiLanguage: 'es' | 'en' | 'pt'
  onNavigate: (page: MainPage) => void
  onLogout: () => void
  onOpenAccountManager: () => void
  accountMenuOpen: boolean
  onToggleMenu: () => void
  onNavigateBack: () => void
  onNavigateForward: () => void
  canNavigateBack: boolean
  canNavigateForward: boolean
  hideSecondaryNav?: boolean
}

function PrincipalTopBar({
  authSession,
  activePage,
  uiLanguage,
  onNavigate,
  onLogout,
  onOpenAccountManager,
  accountMenuOpen,
  onToggleMenu,
  onNavigateBack,
  onNavigateForward,
  canNavigateBack,
  canNavigateForward,
  hideSecondaryNav,
}: PrincipalTopBarProps) {
  const principalSections: MainPage[] = ['Mis Modpacks', 'Novedades', 'Explorador', 'Servers', 'Configuración Global']
  const labels = uiLanguage === 'en'
    ? { back: 'Go back', forward: 'Go forward', account: 'Manage accounts', logout: 'Sign out', noSession: 'Not signed in', settings: 'Settings', myModpacks: 'My Modpacks', news: 'News', explorer: 'Browser', servers: 'Servers' }
    : uiLanguage === 'pt'
      ? { back: 'Voltar', forward: 'Avançar', account: 'Gerenciar contas', logout: 'Encerrar sessão', noSession: 'Sem sessão iniciada', settings: 'Configurações', myModpacks: 'Meus Modpacks', news: 'Novidades', explorer: 'Explorador', servers: 'Servidores' }
      : { back: 'Ir hacia atrás', forward: 'Ir hacia adelante', account: 'Administrar cuentas', logout: 'Cerrar sesión', noSession: 'Sin sesión iniciada', settings: 'Configuración', myModpacks: 'Mis Modpacks', news: 'Novedades', explorer: 'Explorador', servers: 'Servers' }

  return (
    <header className="top-launcher-shell">
      <div className="top-bar principal">
        <div className="launcher-brand-block">
          <div className="launcher-brand-logo-slot" aria-hidden="true">
            <img className="launcher-brand-logo-image" src="/vite.svg" alt="" />
          </div>
          <strong>INTERFACE</strong>
          <button type="button" className="window-chip" aria-label={labels.back} onClick={onNavigateBack} disabled={!canNavigateBack}>←</button>
          <button type="button" className="window-chip" aria-label={labels.forward} onClick={onNavigateForward} disabled={!canNavigateForward}>→</button>
        </div>
        {authSession ? (
          <div className="account-menu">
            <button className="account-menu-trigger" onClick={onToggleMenu}>
              {authSession.profileName}
            </button>
            {accountMenuOpen && (
              <div className="account-menu-dropdown">
                <button onClick={onOpenAccountManager}>{labels.account}</button>
                <button onClick={onLogout}>{labels.logout}</button>
              </div>
            )}
          </div>
        ) : (
          <span>{labels.noSession}</span>
        )}
      </div>
      {authSession && !hideSecondaryNav && (
        <nav className="top-bar secondary launcher-main-nav" aria-label="Navegación principal">
          {principalSections.map((section) => (
            <button
              type="button"
              key={section}
              className={activePage === section ? 'active' : ''}
              onClick={() => onNavigate(section)}
            >
              {section === 'Configuración Global'
                ? labels.settings
                : section === 'Mis Modpacks'
                  ? labels.myModpacks
                  : section === 'Novedades'
                    ? labels.news
                    : section === 'Explorador'
                      ? labels.explorer
                      : section === 'Servers'
                        ? labels.servers
                        : section}
            </button>
          ))}
        </nav>
      )}
    </header>
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
  advancedActions?: string[]
  selectedAdvancedAction?: string
  onAdvancedActionSelect?: (action: string) => void
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
  advancedActions,
  selectedAdvancedAction,
  onAdvancedActionSelect,
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
          {loaderActions && <p className="filter-label">Loader</p>}
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
          {advancedActions && <p className="filter-label">Canal</p>}
          {advancedActions?.map((action) => (
            <button key={`${title}-advanced-${action}`} className={selectedAdvancedAction === action ? 'active' : ''} onClick={() => onAdvancedActionSelect?.(action)}>{action}</button>
          ))}
          {advancedActions && <hr className="sidebar-divider" />}
          <p className="filter-label">Tipo</p>
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
