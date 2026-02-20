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
  targetName: string
  targetGroup: string
  ramMb: number
  copyMods: boolean
  copyWorlds: boolean
  copyResourcepacks: boolean
  copyScreenshots: boolean
  copyLogs: boolean
}
