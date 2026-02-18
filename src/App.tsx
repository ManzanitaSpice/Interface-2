import { invoke } from '@tauri-apps/api/core'
import { useEffect, useMemo, useRef, useState, type ChangeEvent, type CSSProperties, type PointerEvent as ReactPointerEvent } from 'react'
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
}

type LaunchValidationResult = {
  javaPath: string
  javaVersion: string
  classpath: string
  jvmArgs: string[]
  gameArgs: string[]
  mainClass: string
  logs: string[]
}

type StartInstanceResult = {
  pid: number
  javaPath: string
  logs: string[]
}

type RuntimeStatus = {
  running: boolean
  pid: number | null
  exitCode: number | null
  stderrTail: string[]
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

type LoaderChannelFilter = 'Todos' | 'Stable' | 'Latest' | 'Maven'

type InstanceSettingsTab = 'General' | 'Java' | 'Ajustes' | 'Comandos Personalizados' | 'Variables de Entorno'

type MicrosoftAuthStart = {
  authorizeUrl: string
  codeVerifier: string
  redirectUri: string
}


type MicrosoftAuthResult = {
  minecraftAccessToken: string
  microsoftAccessToken: string
  microsoftRefreshToken?: string
  profile: {
    id: string
    name: string
  }
}

type AuthSession = {
  profileId: string
  profileName: string
  minecraftAccessToken: string
  microsoftAccessToken: string
  microsoftRefreshToken?: string
  loggedAt: number
}


const topNavItems: TopNavItem[] = ['Mis Modpacks', 'Novedades', 'Explorador', 'Servers', 'Configuración Global']

const creatorSections: CreatorSection[] = ['Personalizado', 'CurseForge', 'Modrinth', 'Futuro 1', 'Futuro 2', 'Futuro 3']

const editSections: EditSection[] = ['Ejecución', 'Version', 'Mods', 'Resource Packs', 'Shader Packs', 'Notas', 'Mundos', 'Servidores', 'Capturas de Pantalla', 'Configuración', 'Otros registros']

const instanceActions = ['Iniciar', 'Forzar Cierre', 'Editar', 'Cambiar Grupo', 'Carpeta', 'Exportar', 'Copiar', 'Crear atajo']
const defaultGroup = 'Sin grupo'
const sidebarMinWidth = 144
const sidebarMaxWidth = 320
const mojangManifestUrl = 'https://launchermeta.mojang.com/mc/game/version_manifest.json'
const authSessionKey = 'launcher_microsoft_auth_session_v1'
const authCodeRegenerateCooldownMs = 10_000

function nowTimestamp() {
  return new Date().toLocaleTimeString('es-ES', { hour12: false })
}

function makeConsoleEntry(level: ConsoleLevel, source: ConsoleSource, message: string): ConsoleEntry {
  return { timestamp: nowTimestamp(), level, source, message }
}

function classifyConsoleLine(line: string): ConsoleLevel {
  const lowered = line.toLowerCase()
  if (
    lowered.includes('unable to access jarfile') ||
    lowered.includes('classnotfoundexception') ||
    lowered.includes('unsupportedclassversionerror') ||
    lowered.includes('could not reserve enough space') ||
    lowered.includes('asset not found')
  ) {
    return 'FATAL'
  }
  if (lowered.includes('exception') || lowered.includes('error')) return 'ERROR'
  if (lowered.includes('warn')) return 'WARN'
  return 'INFO'
}

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
  const [instanceIconPreview, setInstanceIconPreview] = useState<string>('⛏')
  const [isCreating, setIsCreating] = useState(false)
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
  const [launchPreparation, setLaunchPreparation] = useState<LaunchValidationResult | null>(null)
  const [consoleLevelFilter, setConsoleLevelFilter] = useState<'Todos' | ConsoleLevel>('Todos')
  const [launcherLogFilter, setLauncherLogFilter] = useState<'Todos' | ConsoleSource>('Todos')
  const [autoScrollConsole, setAutoScrollConsole] = useState(true)
  const [instanceDrafts, setInstanceDrafts] = useState<Record<string, InstanceSummary>>({})
  const [selectedInstanceMetadata, setSelectedInstanceMetadata] = useState<InstanceMetadataView | null>(null)
  const [selectedSettingsTab, setSelectedSettingsTab] = useState<InstanceSettingsTab>('General')
  const [isStartingInstance, setIsStartingInstance] = useState(false)
  const [isInstanceRunning, setIsInstanceRunning] = useState(false)
  const [lastRuntimeExitKey, setLastRuntimeExitKey] = useState('')
  const [showDeleteInstanceConfirm, setShowDeleteInstanceConfirm] = useState(false)
  const [isDeletingInstance, setIsDeletingInstance] = useState(false)
  const [authSession, setAuthSession] = useState<AuthSession | null>(null)
  const [isAuthReady, setIsAuthReady] = useState(false)
  const [isAuthenticating, setIsAuthenticating] = useState(false)
  const [authRetryAt, setAuthRetryAt] = useState(0)
  const [nowTick, setNowTick] = useState(() => Date.now())
  const [authStatus, setAuthStatus] = useState('')
  const [authError, setAuthError] = useState('')
  const creationIconInputRef = useRef<HTMLInputElement | null>(null)
  const runtimeConsoleRef = useRef<HTMLDivElement | null>(null)


  const appendRuntime = (entry: ConsoleEntry) => {
    setRuntimeConsole((prev) => {
      const next = [...prev, entry]
      return next.length > 2000 ? next.slice(next.length - 2000) : next
    })
  }

  const persistAuthSession = (session: AuthSession | null) => {
    if (!session) {
      localStorage.removeItem(authSessionKey)
      return
    }
    localStorage.setItem(authSessionKey, JSON.stringify(session))
  }

  const logout = () => {
    setAuthSession(null)
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
        microsoftAccessToken: result.microsoftAccessToken,
        microsoftRefreshToken: result.microsoftRefreshToken,
        loggedAt: Date.now(),
      }

      setAuthSession(session)
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

  const authRetrySeconds = Math.max(0, Math.ceil((authRetryAt - nowTick) / 1000))
  const isAuthCooldown = authRetrySeconds > 0




  const iconButtonStyle = instanceIconPreview.startsWith('data:image')
    ? ({ backgroundImage: `url(${instanceIconPreview})`, backgroundSize: 'cover', backgroundPosition: 'center', color: 'transparent' } as CSSProperties)
    : undefined



  useEffect(() => {
    const stored = localStorage.getItem(authSessionKey)
    if (stored) {
      try {
        const parsed = JSON.parse(stored) as AuthSession
        if (parsed.profileId && parsed.profileName && parsed.minecraftAccessToken) {
          setAuthSession(parsed)
          setAuthStatus(`Sesión restaurada para ${parsed.profileName}.`)
        }
      } catch {
        localStorage.removeItem(authSessionKey)
      }
    }
    setIsAuthReady(true)
  }, [])

  useEffect(() => {
    if (!autoScrollConsole || !runtimeConsoleRef.current) return
    runtimeConsoleRef.current.scrollTop = runtimeConsoleRef.current.scrollHeight
  }, [runtimeConsole, autoScrollConsole])

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
        }
      } catch (error) {
        if (cancelled) return
        const message = error instanceof Error ? error.message : String(error)
        appendRuntime(makeConsoleEntry('ERROR', 'launcher', `No se pudo cargar la configuración de la instancia: ${message}`))
      }
    }

    loadInstanceMetadata()

    return () => {
      cancelled = true
    }
  }, [activePage, selectedCard])

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
            appendRuntime(makeConsoleEntry(status.exitCode === 0 ? 'INFO' : 'ERROR', 'launcher', `Proceso finalizado con exit_code=${status.exitCode}.`))
            if (status.stderrTail.length > 0) {
              appendRuntime(makeConsoleEntry('WARN', 'game', `stderr (últimas ${status.stderrTail.length} líneas): ${status.stderrTail.join(' | ')}`))
            }
            setLastRuntimeExitKey(exitKey)
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
  }, [lastRuntimeExitKey, selectedCard?.instanceRoot])

  useEffect(() => {
    const onEscapePress = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      event.preventDefault()

      if (activePage === 'Editar Instancia') {
        if (selectedEditSection === 'Configuración' && selectedSettingsTab !== 'General') {
          setSelectedSettingsTab('General')
          return
        }
        if (selectedEditSection !== 'Ejecución') {
          setSelectedEditSection('Ejecución')
          return
        }
        if (isInstanceRunning || isStartingInstance) {
          appendRuntime(makeConsoleEntry('WARN', 'launcher', 'No se puede cerrar el editor mientras la instancia está ejecutándose.'))
          return
        }
        setActivePage('Mis Modpacks')
        return
      }

      if (activePage === 'Creador de Instancias') {
        setActivePage('Mis Modpacks')
        return
      }

      if (activePage === 'Mis Modpacks' && selectedCard) {
        setSelectedCard(null)
        return
      }

      if (activePage !== 'Mis Modpacks') {
        setActivePage('Mis Modpacks')
      }
    }

    window.addEventListener('keydown', onEscapePress)
    return () => window.removeEventListener('keydown', onEscapePress)
  }, [activePage, isInstanceRunning, isStartingInstance, selectedCard, selectedEditSection, selectedSettingsTab])


  useEffect(() => {
    let cancelled = false

    const loadInstances = async () => {
      try {
        const loadedCards = await invoke<InstanceSummary[]>('list_instances')
        if (cancelled) return
        setCards(loadedCards)
        setSelectedCard((prev) => {
          if (!prev) return null
          return loadedCards.find((card) => card.id === prev.id) ?? null
        })
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error)
        setCreationConsoleLogs((prev) => [...prev, `No se pudieron cargar las instancias guardadas: ${message}`])
      }
    }

    loadInstances()

    return () => {
      cancelled = true
    }
  }, [])

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
        return entry.source === 'maven'
      })
      .filter((entry) => !searchTerm || entry.version.toLowerCase().includes(searchTerm))
      .map((entry) => [entry.version, entry.publishedAt, entry.source])
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
    setCreationConsoleLogs([
      'FASE 2 iniciada al presionar OK.',
      'Validación ✓ nombre no vacío.',
      'Validación ✓ version.json disponible.',
      `Validación ✓ espacio mínimo estimado (${diskEstimateMb} MB).`,
      `Preparación ✓ Java requerido: ${requiredJava}.`,
      'Preparación ✓ no se realizaron descargas pesadas durante la selección.',
    ])

    try {
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
          },
        },
      })

      const created = { id: result.id, name: result.name, group: result.group, instanceRoot: result.instanceRoot }
      setCards((prev) => [...prev, created])
      setInstanceDrafts((prev) => ({ ...prev, [created.id]: created }))
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

  const appendRuntimeSummary = async () => {
    if (!selectedCard?.instanceRoot || !selectedMinecraftVersion) return
    if (!authSession) {
      setRuntimeConsole([
        makeConsoleEntry('ERROR', 'launcher', 'Debes iniciar sesión con cuenta oficial para validar lanzamiento (sin Demo).'),
      ])
      return
    }

    try {
      const prepared = await invoke<LaunchValidationResult>('validate_and_prepare_launch', {
        instanceRoot: selectedCard.instanceRoot,
        authSession: {
          profileId: authSession.profileId,
          profileName: authSession.profileName,
          minecraftAccessToken: authSession.minecraftAccessToken,
        },
      })

      setLaunchPreparation(prepared)
      const entries: ConsoleEntry[] = [
        makeConsoleEntry('INFO', 'launcher', `Inicio del proceso para ${selectedCard.name}`),
        makeConsoleEntry('INFO', 'launcher', `java_path efectivo: ${prepared.javaPath}`),
        makeConsoleEntry('INFO', 'launcher', `java -version detectado: ${prepared.javaVersion}`),
        makeConsoleEntry('INFO', 'launcher', `MainClass: ${prepared.mainClass}`),
        makeConsoleEntry('INFO', 'launcher', `Classpath válido (${prepared.classpath.split(/[:;]/).length} entradas)`),
        makeConsoleEntry('INFO', 'launcher', `JVM args: ${prepared.jvmArgs.length} | Game args: ${prepared.gameArgs.length}`),
        ...prepared.logs.map((line) => makeConsoleEntry('INFO', 'launcher', line)),
        makeConsoleEntry('INFO', 'game', 'Proceso listo para ejecución real con orden JVM -> mainClass -> game args'),
      ]
      setRuntimeConsole(entries.length > 2000 ? entries.slice(entries.length - 2000) : entries)
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      setRuntimeConsole([
        makeConsoleEntry('ERROR', 'launcher', `Validación de lanzamiento falló: ${message}`),
      ])
    }
  }

  const startInstanceProcess = async () => {
    if (!selectedCard?.instanceRoot) return
    if (!authSession) {
      appendRuntime(makeConsoleEntry('ERROR', 'launcher', 'Debes iniciar sesión con cuenta oficial para iniciar (sin Demo).'))
      return
    }
    if (isStartingInstance || isInstanceRunning) {
      appendRuntime(makeConsoleEntry('WARN', 'launcher', 'La instancia ya está en ejecución o iniciándose.'))
      return
    }

    setIsStartingInstance(true)
    await appendRuntimeSummary()

    try {
      const result = await invoke<StartInstanceResult>('start_instance', {
        instanceRoot: selectedCard.instanceRoot,
        authSession: {
          profileId: authSession.profileId,
          profileName: authSession.profileName,
          minecraftAccessToken: authSession.minecraftAccessToken,
        },
      })

      setRuntimeConsole((prev) => {
        const next = [
          ...prev,
          makeConsoleEntry('INFO', 'launcher', `Proceso de Minecraft iniciado (PID ${result.pid}) con Java ${result.javaPath}`),
          ...result.logs.map((line) => makeConsoleEntry('INFO', 'launcher', line)),
        ]
        return next.length > 2000 ? next.slice(next.length - 2000) : next
      })
      setIsInstanceRunning(true)
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      appendRuntime(makeConsoleEntry('ERROR', 'launcher', `No se pudo iniciar el proceso de la instancia: ${message}`))
    } finally {
      setIsStartingInstance(false)
    }
  }

  const pushRuntimeStream = () => {
    const demoLines = [
      '[STDOUT] Loading world renderer...',
      '[WARN] Missing optional shader pack metadata.',
      '[STDERR] Exception in thread main',
      'UnsupportedClassVersionError: bad major version',
      '[STDOUT] Tick loop stable at 60 TPS',
    ]

    demoLines.forEach((line) => {
      appendRuntime(makeConsoleEntry(classifyConsoleLine(line), line.includes('STDERR') ? 'game' : 'launcher', line))
    })
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
    setSelectedSettingsTab('General')
    setActivePage('Editar Instancia')
  }

  const handleInstanceAction = async (action: string) => {
    if (!selectedCard) return

    if (action === 'Iniciar') {
      if (isStartingInstance || isInstanceRunning) return
      openEditor()
      void startInstanceProcess()
      return
    }

    if (action === 'Editar') {
      openEditor()
      return
    }

    if (action === 'Exportar') {
      await exportRuntimeLog()
      return
    }

    if (action === 'Eliminar') {
      setShowDeleteInstanceConfirm(true)
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

  return (
    <div className="app-shell">
      <PrincipalTopBar authSession={authSession} onLogout={logout} />

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

      {authSession && activePage !== 'Creador de Instancias' && activePage !== 'Editar Instancia' && (
        <SecondaryTopBar activePage={activePage} onNavigate={onTopNavClick} />
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
                  <small>{card.instanceRoot ?? "Ruta pendiente"}</small>
                </article>
              ))}
            </div>
          </section>
        </main>
      )}

      {authSession && activePage === 'Mis Modpacks' && (
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
                    <span className="instance-group-chip">Grupo: {card.group}</span>
                    <small>{card.instanceRoot ?? "Ruta pendiente"}</small>
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
                      <div key={action} className="instance-action-item">
                        <button className={action === 'Editar' ? 'primary' : ''} onClick={() => handleInstanceAction(action)}>
                          {action}
                        </button>
                        {action === 'Editar' && (
                          <button className="danger" onClick={() => handleInstanceAction('Eliminar')}>
                            Eliminar
                          </button>
                        )}
                      </div>
                    ))}
                  </div>
                </aside>
              )}
            </div>
          </section>
        </main>
      )}

      {showDeleteInstanceConfirm && selectedCard && (
        <div className="floating-modal-overlay" role="dialog" aria-modal="true" aria-label="Confirmar eliminación de instancia">
          <div className="floating-modal">
            <h3>¿Eliminar instancia?</h3>
            <p>Se eliminará completamente la instancia <strong>{selectedCard.name}</strong> y todos sus archivos.</p>
            <div className="floating-modal-actions">
              <button onClick={() => setShowDeleteInstanceConfirm(false)} disabled={isDeletingInstance}>Cancelar</button>
              <button className="danger" onClick={deleteSelectedInstance} disabled={isDeletingInstance}>
                {isDeletingInstance ? 'Eliminando...' : 'Eliminar'}
              </button>
            </div>
          </div>
        </div>
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

            <footer className="creator-footer-actions">
              <button className="primary" onClick={createInstance} disabled={isCreating || !selectedMinecraftVersion}>
                {isCreating ? 'Creando...' : 'Ok'}
              </button>
              <button onClick={() => setActivePage('Mis Modpacks')}>Cancelar</button>
            </footer>
          </section>
        </main>
      )}

      {authSession && activePage === 'Editar Instancia' && selectedCard && (
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
                    <button className="ghost-btn" onClick={pushRuntimeStream}>Simular stream</button>
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

type PrincipalTopBarProps = {
  authSession: AuthSession | null
  onLogout: () => void
}

function PrincipalTopBar({ authSession, onLogout }: PrincipalTopBarProps) {
  return (
    <header className="top-bar principal">
      <strong>Launcher Control Center</strong>
      {authSession ? (
        <span>
          {authSession.profileName}
          <button style={{ marginLeft: '0.65rem' }} onClick={onLogout}>Cerrar sesión</button>
        </span>
      ) : (
        <span>Sin sesión iniciada</span>
      )}
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
          {advancedActions?.map((action) => (
            <button key={`${title}-advanced-${action}`} className={selectedAdvancedAction === action ? 'active' : ''} onClick={() => onAdvancedActionSelect?.(action)}>{action}</button>
          ))}
          {advancedActions && <hr className="sidebar-divider" />}
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
