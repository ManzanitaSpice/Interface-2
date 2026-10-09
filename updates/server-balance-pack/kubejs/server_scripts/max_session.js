// Server-only KubeJS command: /max <player> and /max off <player>
// The boost survives death, but it is removed when the player disconnects.

const ForgeRegistries = Java.loadClass('net.minecraftforge.registries.ForgeRegistries')
const ResourceLocation = Java.loadClass('net.minecraft.resources.ResourceLocation')
const StringArgumentType = Java.loadClass('com.mojang.brigadier.arguments.StringArgumentType')
const IronMagicHelper = Java.loadClass('io.redspace.ironsspellbooks.api.magic.MagicHelper')
const MnsLoad = Java.loadClass('com.robertx22.mine_and_slash.uncommon.datasaving.Load')
const MnsModType = Java.loadClass('com.robertx22.mine_and_slash.uncommon.enumclasses.ModType')

const MAX_SESSION_ACTIVE = 'interfaceMaxSessionActive'
const MAX_SESSION_ORIGINAL = 'interfaceMaxOriginal_'
const MAX_SESSION_STATS = [
  ['interface_max_mana_regen', 'mana_regen', 999],
  ['interface_max_cast_speed', 'cast_speed', 300],
  ['interface_max_cooldown', 'cdr', 75]
]

const MAX_SESSION_LEGACY_STATS = [
  'interface_max_spell_damage',
  'interface_max_mana'
]

const MAX_SESSION_ATTRIBUTES = [
  ['ars_nouveau:ars_nouveau.perk.mana_regen', 999],
  ['irons_spellbooks:mana_regen', 100],
  ['irons_spellbooks:cooldown_reduction', 100],
  ['irons_spellbooks:cast_time_reduction', 100]
]

// Attributes used by older versions of this script. They are restored once so an
// existing player cannot keep an offensive, defensive or summon-power boost.
const MAX_SESSION_LEGACY_POWER_ATTRIBUTES = [
  'ars_nouveau:ars_nouveau.perk.max_mana',
  'irons_spellbooks:max_mana',
  'ars_nouveau:ars_nouveau.perk.spell_damage',
  'ars_nouveau:ars_nouveau.perk.warding',
  'irons_spellbooks:spell_power',
  'irons_spellbooks:spell_resist',
  'irons_spellbooks:summon_damage',
  'irons_spellbooks:casting_movespeed',
  'irons_spellbooks:fire_spell_power',
  'irons_spellbooks:ice_spell_power',
  'irons_spellbooks:lightning_spell_power',
  'irons_spellbooks:holy_spell_power',
  'irons_spellbooks:ender_spell_power',
  'irons_spellbooks:blood_spell_power',
  'irons_spellbooks:evocation_spell_power',
  'irons_spellbooks:nature_spell_power',
  'irons_spellbooks:eldritch_spell_power',
  'irons_spellbooks:fire_magic_resist',
  'irons_spellbooks:ice_magic_resist',
  'irons_spellbooks:lightning_magic_resist',
  'irons_spellbooks:holy_magic_resist',
  'irons_spellbooks:ender_magic_resist',
  'irons_spellbooks:blood_magic_resist',
  'irons_spellbooks:evocation_magic_resist',
  'irons_spellbooks:nature_magic_resist',
  'irons_spellbooks:eldritch_magic_resist'
]

function maxSessionSnapshotKey(attributeId) {
  return MAX_SESSION_ORIGINAL + attributeId.replace(/[^a-zA-Z0-9_]/g, '_')
}

function maxSessionAttribute(player, attributeId) {
  var attribute = ForgeRegistries.ATTRIBUTES.getValue(new ResourceLocation(attributeId))
  return attribute == null ? null : player.getAttribute(attribute)
}

function maxSessionApplyAttributes(player, takeSnapshot) {
  var data = player.persistentData
  MAX_SESSION_ATTRIBUTES.forEach(pair => {
    try {
      var attributeInstance = maxSessionAttribute(player, pair[0])
      if (attributeInstance == null) return
      var snapshotKey = maxSessionSnapshotKey(pair[0])
      if (takeSnapshot && !data.contains(snapshotKey)) {
        data.putDouble(snapshotKey, attributeInstance.getBaseValue())
      }
      attributeInstance.setBaseValue(pair[1])
    } catch (error) {
      console.warn(`[max] No se pudo aplicar ${pair[0]}: ${error}`)
    }
  })
}

function maxSessionRestoreAttributeIds(player, attributeIds) {
  var data = player.persistentData
  attributeIds.forEach(attributeId => {
    try {
      var snapshotKey = maxSessionSnapshotKey(attributeId)
      var attributeInstance = maxSessionAttribute(player, attributeId)
      if (attributeInstance != null && data.contains(snapshotKey)) {
        attributeInstance.setBaseValue(data.getDouble(snapshotKey))
      }
      data.remove(snapshotKey)
    } catch (error) {
      console.warn(`[max] No se pudo restaurar ${attributeId}: ${error}`)
    }
  })
}

function maxSessionRestoreLegacyPower(player) {
  maxSessionRestoreAttributeIds(player, MAX_SESSION_LEGACY_POWER_ATTRIBUTES)
  try {
    var exactStats = MnsLoad.Unit(player).getCustomExactStats()
    MAX_SESSION_LEGACY_STATS.forEach(statKey => exactStats.removeExactStat(statKey))
    MnsLoad.Unit(player).setEquipsChanged()
  } catch (error) {
    console.warn(`[max] Mine and Slash no pudo retirar el poder mágico antiguo: ${error}`)
  }
}

function maxSessionApplyMns(player) {
  try {
    var mnsUnit = MnsLoad.Unit(player)
    var exactStats = mnsUnit.getCustomExactStats()
    MAX_SESSION_LEGACY_STATS.forEach(statKey => exactStats.removeExactStat(statKey))
    MAX_SESSION_STATS.forEach(entry => {
      exactStats.removeExactStat(entry[0])
      exactStats.addExactStat(entry[0], entry[1], entry[2], MnsModType.FLAT)
    })
    mnsUnit.setEquipsChanged()
  } catch (error) {
    console.warn(`[max] Mine and Slash no pudo actualizarse: ${error}`)
  }
}

function maxSessionClearIronCooldowns(player) {
  try {
    IronMagicHelper.MAGIC_MANAGER.clearCooldowns(player)
  } catch (error) {
    console.warn(`[max] Iron's Spells no pudo limpiar cooldowns: ${error}`)
  }
}

function maxSessionEnable(player, refill) {
  var firstApply = !player.persistentData.getBoolean(MAX_SESSION_ACTIVE)
  maxSessionRestoreLegacyPower(player)
  maxSessionApplyAttributes(player, firstApply)
  maxSessionApplyMns(player)
  player.persistentData.putBoolean(MAX_SESSION_ACTIVE, true)
  if (refill) {
    maxSessionClearIronCooldowns(player)
    player.server.runCommandSilent(`effect clear ${player.username} irons_spellbooks:fortify`)
  }
}

function maxSessionDisable(player) {
  var data = player.persistentData
  maxSessionRestoreAttributeIds(player, MAX_SESSION_ATTRIBUTES.map(pair => pair[0]))
  maxSessionRestoreLegacyPower(player)

  try {
    var exactStats = MnsLoad.Unit(player).getCustomExactStats()
    MAX_SESSION_STATS.forEach(entry => exactStats.removeExactStat(entry[0]))
    MAX_SESSION_LEGACY_STATS.forEach(statKey => exactStats.removeExactStat(statKey))
    MnsLoad.Unit(player).setEquipsChanged()
  } catch (error) {
    console.warn(`[max] Mine and Slash no pudo restaurarse: ${error}`)
  }

  data.remove(MAX_SESSION_ACTIVE)
}

function maxSessionFindPlayer(source, name) {
  return source.server.playerList.getPlayerByName(name)
}

ServerEvents.commandRegistry(event => {
  var Commands = event.commands
  var playerArgument = () => Commands.argument('player', StringArgumentType.word())

  event.register(
    Commands.literal('max')
      .requires(source => source.hasPermission(2))
      .then(playerArgument().executes(context => {
        var name = StringArgumentType.getString(context, 'player')
        var player = maxSessionFindPlayer(context.source, name)
        if (player == null) {
          context.source.sendFailure(Component.literal(`No se encontró al jugador conectado: ${name}`))
          return 0
        }
        maxSessionEnable(player, true)
        context.source.sendSuccess(Component.literal(`/max activado para ${player.username} hasta que salga del servidor.`), true)
        return 1
      }))
      .then(Commands.literal('off').then(playerArgument().executes(context => {
        var name = StringArgumentType.getString(context, 'player')
        var player = maxSessionFindPlayer(context.source, name)
        if (player == null) {
          context.source.sendFailure(Component.literal(`No se encontró al jugador conectado: ${name}`))
          return 0
        }
        maxSessionDisable(player)
        context.source.sendSuccess(Component.literal(`/max desactivado para ${player.username}.`), true)
        return 1
      })))
  )
})

PlayerEvents.tick(event => {
  var player = event.player
  if (player.persistentData.getBoolean(MAX_SESSION_ACTIVE) && player.age % 100 == 0) {
    maxSessionEnable(player, false)
  }
})

PlayerEvents.respawned(event => {
  if (event.player.persistentData.getBoolean(MAX_SESSION_ACTIVE)) {
    maxSessionEnable(event.player, true)
  }
})

PlayerEvents.loggedOut(event => {
  if (event.player.persistentData.getBoolean(MAX_SESSION_ACTIVE)) {
    maxSessionDisable(event.player)
  }
})

PlayerEvents.loggedIn(event => {
  // A stale marker can only remain after a crash/restart; never reactivate it automatically.
  if (event.player.persistentData.getBoolean(MAX_SESSION_ACTIVE)) {
    maxSessionDisable(event.player)
  }
})
