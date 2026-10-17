// Starts Majrusz's native Blood Moon when a player manually activates the
// Horror Elements atmosphere block, while undoing its time-of-day reset.

const AtmosphereBuiltInRegistries = Java.loadClass('net.minecraft.core.registries.BuiltInRegistries')
const MajruszBloodMoon = Java.loadClass('com.majruszsdifficulty.bloodmoon.BloodMoonHelper')

const ATMOSPHERE_BLOCK_ID = 'horror_element_mod:atmosphere_block'
const ATMOSPHERE_ACTION_WINDOW_MS = 5000
const ATMOSPHERE_PLAYER_RADIUS_SQUARED = 4096
const ATMOSPHERE_BLOOD_MOON_COOLDOWN_MS = 5 * 60 * 1000
const ATMOSPHERE_RECENT_ACTIONS = []
const ATMOSPHERE_POWERED_STATE = Object.create(null)
const ATMOSPHERE_PENDING_TIME_RESTORES = Object.create(null)
const ATMOSPHERE_PENDING_BLOOD_MOONS = []
const ATMOSPHERE_LAST_QUEUED = Object.create(null)
var ATMOSPHERE_DAY_RESET_REENABLED = false
var ATMOSPHERE_PLAYER_ACTION_WARNING_SHOWN = false
var ATMOSPHERE_BLOOD_MOON_WAS_ACTIVE = false
var ATMOSPHERE_BLOOD_MOON_COOLDOWN_UNTIL = 0

function atmosphereDimensionId(level) {
  return String(level.dimension)
}

function atmosphereBlockKey(level, pos) {
  return `${atmosphereDimensionId(level)}|${pos.asLong()}`
}

function atmospherePlayerName(player) {
  var name = player.username
  if (name == null) name = player.getGameProfile().getName()
  return String(name)
}

function atmosphereRememberPlayerAction(event) {
  try {
    var player = event.player
    if (player == null) return

    var level = player.level
    if (level.isClientSide()) return

    ATMOSPHERE_RECENT_ACTIONS.push({
      name: atmospherePlayerName(player),
      dimension: atmosphereDimensionId(level),
      x: player.getX(),
      y: player.getY(),
      z: player.getZ(),
      time: Date.now()
    })

    while (ATMOSPHERE_RECENT_ACTIONS.length > 64) {
      ATMOSPHERE_RECENT_ACTIONS.shift()
    }
  } catch (error) {
    if (!ATMOSPHERE_PLAYER_ACTION_WARNING_SHOWN) {
      ATMOSPHERE_PLAYER_ACTION_WARNING_SHOWN = true
      console.warn(`[atmosphere_bloodmoon] No se pudo registrar al jugador: ${error}`)
    }
  }
}

function atmosphereScheduleTimeRestore(level) {
  var dimension = atmosphereDimensionId(level)
  if (ATMOSPHERE_PENDING_TIME_RESTORES[dimension] != null) return

  ATMOSPHERE_PENDING_TIME_RESTORES[dimension] = {
    level: level,
    dayTime: Number(level.getDayTime()),
    gameTime: Number(level.getGameTime())
  }
}

function atmosphereDistanceSquared(pos, x, y, z) {
  var dx = x - (pos.getX() + 0.5)
  var dy = y - (pos.getY() + 0.5)
  var dz = z - (pos.getZ() + 0.5)
  return dx * dx + dy * dy + dz * dz
}

function atmosphereFindActivator(level, pos) {
  var dimension = atmosphereDimensionId(level)
  var now = Date.now()
  var recentNearby = null

  for (var i = 0; i < ATMOSPHERE_RECENT_ACTIONS.length; i++) {
    var action = ATMOSPHERE_RECENT_ACTIONS[i]
    if (action.dimension !== dimension || now - action.time > ATMOSPHERE_ACTION_WINDOW_MS) continue
    if (atmosphereDistanceSquared(pos, action.x, action.y, action.z) <= ATMOSPHERE_PLAYER_RADIUS_SQUARED &&
      (recentNearby == null || action.time > recentNearby.time)) {
      recentNearby = action
    }
  }

  return recentNearby == null ? null : recentNearby.name
}

function atmosphereQueueBloodMoon(level, pos, activator) {
  if (MajruszBloodMoon.isActive()) {
    ATMOSPHERE_BLOOD_MOON_WAS_ACTIVE = true
    return
  }
  if (Date.now() < ATMOSPHERE_BLOOD_MOON_COOLDOWN_UNTIL) return

  var key = atmosphereBlockKey(level, pos)
  var now = Date.now()
  if (ATMOSPHERE_LAST_QUEUED[key] != null && now - ATMOSPHERE_LAST_QUEUED[key] < 1500) return

  ATMOSPHERE_LAST_QUEUED[key] = now
  ATMOSPHERE_PENDING_BLOOD_MOONS.push({
    activator: activator,
    lastAttempt: 0
  })
  console.info(`[atmosphere_bloodmoon] Activacion manual detectada: ${activator}`)
}

BlockEvents.rightClicked(event => {
  atmosphereRememberPlayerAction(event)

  try {
    if (event.player == null || String(event.block.id) !== ATMOSPHERE_BLOCK_ID) return

    var level = event.player.level
    if (level.isClientSide()) return

    atmosphereScheduleTimeRestore(level)
    atmosphereQueueBloodMoon(level, event.block.pos, atmospherePlayerName(event.player))
  } catch (error) {
    console.error(`[atmosphere_bloodmoon] No se pudo procesar el uso directo del bloque: ${error}`)
  }
})

BlockEvents.placed(atmosphereRememberPlayerAction)
BlockEvents.broken(event => {
  if (String(event.block.id) !== ATMOSPHERE_BLOCK_ID || event.player == null) return

  try {
    var level = event.player.level
    atmosphereScheduleTimeRestore(level)
    delete ATMOSPHERE_POWERED_STATE[atmosphereBlockKey(level, event.block.pos)]
  } catch (error) {
    console.error(`[atmosphere_bloodmoon] No se pudo preparar la restauracion de hora: ${error}`)
  }
})

function atmosphereCheckPower(level, pos, seenPositions) {
  var blockId = String(AtmosphereBuiltInRegistries.BLOCK.getKey(level.getBlockState(pos).getBlock()))
  if (blockId !== ATMOSPHERE_BLOCK_ID) return

  var key = atmosphereBlockKey(level, pos)
  if (seenPositions[key]) return
  seenPositions[key] = true

  var powered = level.getBestNeighborSignal(pos) > 0
  var hasPreviousState = Object.prototype.hasOwnProperty.call(ATMOSPHERE_POWERED_STATE, key)
  var wasPowered = ATMOSPHERE_POWERED_STATE[key] === true
  ATMOSPHERE_POWERED_STATE[key] = powered

  if (hasPreviousState && powered === wasPowered) return

  // The mod changes day time on either redstone transition. Save the current
  // clock before its neighbor callback runs, then restore it on the next tick.
  atmosphereScheduleTimeRestore(level)

  // A natural signal may update the block, but it must not summon a Blood Moon.
  // On first seeing a powered block, require the same recent player evidence.
  if (!powered) return

  var activator = atmosphereFindActivator(level, pos)
  if (activator == null) return

  atmosphereQueueBloodMoon(level, pos, activator)
}

ForgeEvents.onEvent('net.neoforged.neoforge.event.level.BlockEvent$NeighborNotifyEvent', event => {
  var level = event.getLevel()
  if (level.isClientSide()) return

  var pos = event.getPos()
  var seenPositions = Object.create(null)

  atmosphereCheckPower(level, pos, seenPositions)

  var directions = event.getNotifiedSides().iterator()
  while (directions.hasNext()) {
    atmosphereCheckPower(level, pos.relative(directions.next()), seenPositions)
  }
})

function atmosphereRestorePendingTimes() {
  for (var dimension in ATMOSPHERE_PENDING_TIME_RESTORES) {
    var snapshot = ATMOSPHERE_PENDING_TIME_RESTORES[dimension]
    try {
      var elapsed = Math.max(0, Number(snapshot.level.getGameTime()) - snapshot.gameTime)
      snapshot.level.setDayTime(snapshot.dayTime + elapsed)
    } catch (error) {
      console.error(`[atmosphere_bloodmoon] No se pudo restaurar la hora de ${dimension}: ${error}`)
    }
    delete ATMOSPHERE_PENDING_TIME_RESTORES[dimension]
  }
}

ServerEvents.tick(event => {
  if (!ATMOSPHERE_DAY_RESET_REENABLED) {
    // Clear a previous HMdayreset false state; leaving it disabled spams a
    // warning on every block update and prevents Horror Elements' procedure.
    event.server.runCommandSilent('HMdayreset true')
    ATMOSPHERE_DAY_RESET_REENABLED = true
  }

  atmosphereRestorePendingTimes()

  var bloodMoonActive = MajruszBloodMoon.isActive()
  if (bloodMoonActive) {
    ATMOSPHERE_BLOOD_MOON_WAS_ACTIVE = true
    ATMOSPHERE_PENDING_BLOOD_MOONS.length = 0
    return
  }

  if (ATMOSPHERE_BLOOD_MOON_WAS_ACTIVE) {
    ATMOSPHERE_BLOOD_MOON_WAS_ACTIVE = false
    ATMOSPHERE_BLOOD_MOON_COOLDOWN_UNTIL = Date.now() + ATMOSPHERE_BLOOD_MOON_COOLDOWN_MS
    ATMOSPHERE_PENDING_BLOOD_MOONS.length = 0
    console.info('[atmosphere_bloodmoon] La Blood Moon termino; cooldown de 5 minutos iniciado.')
    return
  }

  if (Date.now() < ATMOSPHERE_BLOOD_MOON_COOLDOWN_UNTIL) {
    ATMOSPHERE_PENDING_BLOOD_MOONS.length = 0
    return
  }

  if (ATMOSPHERE_PENDING_BLOOD_MOONS.length === 0) return

  var activation = ATMOSPHERE_PENDING_BLOOD_MOONS[0]
  var now = Date.now()
  if (now - activation.lastAttempt < 1000) return
  activation.lastAttempt = now

  try {
    if (!MajruszBloodMoon.start()) {
      // If Majrusz rejects a daytime start, retain the activation and retry
      // periodically until its normal start window. No artificial time skip.
      if (!MajruszBloodMoon.isValidDayTime()) return

      ATMOSPHERE_PENDING_BLOOD_MOONS.shift()
      console.warn(`[atmosphere_bloodmoon] Majrusz no inicio la Blood Moon solicitada por ${activation.activator}.`)
      return
    }

    ATMOSPHERE_PENDING_BLOOD_MOONS.shift()
    event.server.runCommandSilent('title @a times 10 80 20')
    event.server.runCommandSilent(`title @a title ${JSON.stringify({
      text: `¡Blood Moon activada por ${activation.activator}!`,
      color: 'dark_red',
      bold: true
    })}`)
    console.info(`[atmosphere_bloodmoon] Blood Moon iniciada por ${activation.activator}.`)
  } catch (error) {
    ATMOSPHERE_PENDING_BLOOD_MOONS.shift()
    console.error(`[atmosphere_bloodmoon] No se pudo iniciar la Blood Moon: ${error}`)
  }
})
