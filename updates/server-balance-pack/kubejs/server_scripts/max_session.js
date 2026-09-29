// Server-only KubeJS command: /max <player> and /max off <player>
// The boost survives death, but it is removed when the player disconnects.

const ForgeRegistries = Java.loadClass('net.minecraftforge.registries.ForgeRegistries')
const ResourceLocation = Java.loadClass('net.minecraft.resources.ResourceLocation')
const StringArgumentType = Java.loadClass('com.mojang.brigadier.arguments.StringArgumentType')
const ArsCapabilities = Java.loadClass('com.hollingsworth.arsnouveau.setup.registry.CapabilityRegistry')
const MnsLoad = Java.loadClass('com.robertx22.mine_and_slash.uncommon.datasaving.Load')
const MnsModType = Java.loadClass('com.robertx22.mine_and_slash.uncommon.enumclasses.ModType')
const MnsResourceType = Java.loadClass('com.robertx22.mine_and_slash.saveclasses.unit.ResourceType')

const MAX_SESSION_ACTIVE = 'interfaceMaxSessionActive'
const MAX_SESSION_ORIGINAL = 'interfaceMaxOriginal_'
const MAX_SESSION_STATS = [
  ['interface_max_mana', 'mana', 999],
  ['interface_max_mana_regen', 'mana_regen', 999],
  ['interface_max_spell_damage', 'spell_all_damage', 999],
  ['interface_max_cast_speed', 'cast_speed', 300],
  ['interface_max_cooldown', 'cdr', 75]
]

const MAX_SESSION_ATTRIBUTES = [
  ['ars_nouveau:ars_nouveau.perk.max_mana', 999],
  ['ars_nouveau:ars_nouveau.perk.mana_regen', 999],
  ['ars_nouveau:ars_nouveau.perk.spell_damage', 999],
  ['irons_spellbooks:max_mana', 999],
  ['irons_spellbooks:mana_regen', 100],
  ['irons_spellbooks:cooldown_reduction', 100],
  ['irons_spellbooks:spell_power', 100],
  ['irons_spellbooks:cast_time_reduction', 100],
  ['irons_spellbooks:summon_damage', 100],
  ['irons_spellbooks:casting_movespeed', 100]
]

function maxSessionSnapshotKey(attributeId) {
  return MAX_SESSION_ORIGINAL + attributeId.replace(/[^a-zA-Z0-9_]/g, '_')
}

function maxSessionAttribute(player, attributeId) {
  const attribute = ForgeRegistries.ATTRIBUTES.getValue(new ResourceLocation(attributeId))
  return attribute == null ? null : player.getAttribute(attribute)
}

function maxSessionApplyAttributes(player, takeSnapshot) {
  const data = player.persistentData
  MAX_SESSION_ATTRIBUTES.forEach(pair => {
    try {
      const instance = maxSessionAttribute(player, pair[0])
      if (instance == null) return
      const snapshotKey = maxSessionSnapshotKey(pair[0])
      if (takeSnapshot && !data.contains(snapshotKey)) {
        data.putDouble(snapshotKey, instance.getBaseValue())
      }
      instance.setBaseValue(pair[1])
    } catch (error) {
      console.warn(`[max] No se pudo aplicar ${pair[0]}: ${error}`)
    }
  })
}

function maxSessionApplyMns(player) {
  try {
    const unit = MnsLoad.Unit(player)
    const exactStats = unit.getCustomExactStats()
    MAX_SESSION_STATS.forEach(entry => {
      exactStats.removeExactStat(entry[0])
      exactStats.addExactStat(entry[0], entry[1], entry[2], MnsModType.FLAT)
    })
    unit.setEquipsChanged()
    unit.getResources().restore(player, MnsResourceType.mana, 999999)
  } catch (error) {
    console.warn(`[max] Mine and Slash no pudo actualizarse: ${error}`)
  }
}

function maxSessionFillMana(player) {
  try {
    const mana = ArsCapabilities.getMana(player).orElse(null)
    if (mana != null) mana.setMana(999)
  } catch (error) {
    console.warn(`[max] Ars Nouveau no pudo rellenar maná: ${error}`)
  }

  try {
    player.server.runCommandSilent(`mana set ${player.username} 999`)
  } catch (error) {
    console.warn(`[max] Iron's Spells no pudo rellenar maná: ${error}`)
  }

  maxSessionApplyMns(player)
}

function maxSessionEnable(player, refill) {
  const firstApply = !player.persistentData.getBoolean(MAX_SESSION_ACTIVE)
  maxSessionApplyAttributes(player, firstApply)
  maxSessionApplyMns(player)
  player.persistentData.putBoolean(MAX_SESSION_ACTIVE, true)
  if (refill) maxSessionFillMana(player)
}

function maxSessionDisable(player) {
  const data = player.persistentData
  MAX_SESSION_ATTRIBUTES.forEach(pair => {
    try {
      const snapshotKey = maxSessionSnapshotKey(pair[0])
      const instance = maxSessionAttribute(player, pair[0])
      if (instance != null && data.contains(snapshotKey)) {
        instance.setBaseValue(data.getDouble(snapshotKey))
      }
      data.remove(snapshotKey)
    } catch (error) {
      console.warn(`[max] No se pudo restaurar ${pair[0]}: ${error}`)
    }
  })

  try {
    const exactStats = MnsLoad.Unit(player).getCustomExactStats()
    MAX_SESSION_STATS.forEach(entry => exactStats.removeExactStat(entry[0]))
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
  const Commands = event.commands
  const playerArgument = () => Commands.argument('player', StringArgumentType.word())

  event.register(
    Commands.literal('max')
      .requires(source => source.hasPermission(2))
      .then(playerArgument().executes(context => {
        const name = StringArgumentType.getString(context, 'player')
        const player = maxSessionFindPlayer(context.source, name)
        if (player == null) {
          context.source.sendFailure(Component.literal(`No se encontró al jugador conectado: ${name}`))
          return 0
        }
        maxSessionEnable(player, true)
        context.source.sendSuccess(Component.literal(`/max activado para ${player.username} hasta que salga del servidor.`), true)
        return 1
      }))
      .then(Commands.literal('off').then(playerArgument().executes(context => {
        const name = StringArgumentType.getString(context, 'player')
        const player = maxSessionFindPlayer(context.source, name)
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
  const player = event.player
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
