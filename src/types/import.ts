export type DetectedInstance = {
  id: string
  name: string
  sourceLauncher: string
  sourcePath: string
  minecraftVersion: string
  loader: string
  loaderVersion: string
  format: string
  iconPath?: string | null
  modsCount?: number | null
  sizeMb?: number | null
  lastPlayed?: string | null
  importable: boolean
  importWarnings: string[]
}

export type ImportRequest = {
  detectedInstanceId: string
  sourcePath: string
  targetName: string
  targetGroup: string
  minecraftVersion: string
  loader: string
  loaderVersion: string
  ramMb: number
  copyMods: boolean
  copyWorlds: boolean
  copyResourcepacks: boolean
  copyScreenshots: boolean
  copyLogs: boolean
}

export type ImportAction = 'ejecutar' | 'clonar' | 'migrar'
