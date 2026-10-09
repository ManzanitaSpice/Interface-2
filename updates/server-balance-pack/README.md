# Paquete de balance del servidor

Este paquete es exclusivamente del servidor. Los jugadores no necesitan instalar estos archivos.

## Instalación

1. Copia `kubejs/server_scripts/max_session.js` y `kubejs/startup_scripts/max_iron_spell_events.js` dentro de las carpetas equivalentes de la instancia del servidor.
2. Copia `config/incontrol/spawn.json` dentro del mismo directorio de la instancia del servidor.
3. Reinicia el servidor. Para retocar solamente InControl también puedes usar `/incontrol reload`.

## Comando `/max`

- `/max ManzanitaSpace` activa los valores máximos mientras esa cuenta siga conectada.
- El efecto se repone después de morir.
- Al desconectarse se restauran los valores anteriores; al volver a entrar hay que ejecutar el comando otra vez.
- `/max off ManzanitaSpace` lo desactiva manualmente.
- Requiere nivel de permiso 2 (operador).

El comando sólo modifica tres categorías: regeneración de maná, velocidad de casteo y reducción de cooldown. No aumenta ni rellena el maná, y tampoco aumenta daño, poder de hechizos, nivel de buffs, summons, warding ni resistencias. Mine and Slash usa 300 de velocidad de casteo y 75 de reducción de enfriamiento, sus topes reales.

Al migrar desde una versión anterior, el script restaura automáticamente el maná máximo y los atributos ofensivos o defensivos que aquella versión hubiera guardado. También elimina una Fortificación antigua al activar `/max`, para que no permanezcan corazones creados con el poder mágico anterior; después se puede volver a lanzar normalmente.

Al activar `/max`, los cooldowns existentes de Iron's Spells se limpian. Mientras siga activo, el evento de cooldown se cancela antes de que el mod lo registre. La reducción de casteo permanece en el máximo admitido por Iron's Spells y se aplica a los nuevos lanzamientos. El archivo de `startup_scripts` requiere reiniciar completamente el servidor; `/reload` no basta para registrar ese evento.

## InControl

La configuración sólo limita intentos nuevos de aparición natural cuando ya existe una cantidad alta de entidades por jugador. No elimina entidades existentes y no bloquea invocaciones, huevos, spawners, estructuras, comandos, crianza ni jefes.

Los topes son conservadores: 60 hostiles, 30 pasivos y 110 entidades totales por jugador para apariciones naturales. Si el servidor ya tiene otro `spawn.json`, combina estas reglas en vez de reemplazarlo sin revisar.
