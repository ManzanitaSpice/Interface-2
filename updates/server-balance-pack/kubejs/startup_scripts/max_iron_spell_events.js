// Prevent Iron's Spells from creating new cooldowns while /max is active.
// Startup scripts require a full server restart after installation or changes.

const MAX_IRON_EVENT_ACTIVE_KEY = 'interfaceMaxSessionActive'

ForgeEvents.onEvent('io.redspace.ironsspellbooks.api.events.SpellCooldownAddedEvent$Pre', event => {
  var eventPlayer = event.entity
  if (eventPlayer != null && eventPlayer.persistentData.getBoolean(MAX_IRON_EVENT_ACTIVE_KEY)) {
    event.setEffectiveCooldown(0)
    event.setCanceled(true)
  }
})
