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

export type ImportAction = 'crear_atajo' | 'clonar' | 'migrar' | 'eliminar_instancia'

export type ImportExecutionProgress = {
  instanceId: string
  instanceName: string
  step: string
  stepIndex: number
  totalSteps: number
  completed: number
  total: number
  message: string
}

export type ImportActionRequest = {
  detectedInstanceId: string
  sourcePath: string
  targetName: string
  targetGroup: string
  minecraftVersion: string
  loader: string
  loaderVersion: string
  sourceLauncher: string
  action: ImportAction | 'abrir_carpeta' | 'abrir_origen'
}
