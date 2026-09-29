# Paquete de balance del servidor

Este paquete es exclusivamente del servidor. Los jugadores no necesitan instalar estos archivos.

## Instalación

1. Copia `kubejs/server_scripts/max_session.js` dentro del mismo directorio de la instancia del servidor.
2. Copia `config/incontrol/spawn.json` dentro del mismo directorio de la instancia del servidor.
3. Reinicia el servidor. Para retocar solamente InControl también puedes usar `/incontrol reload`.

## Comando `/max`

- `/max ManzanitaSpace` activa los valores máximos mientras esa cuenta siga conectada.
- El efecto se repone después de morir.
- Al desconectarse se restauran los valores anteriores; al volver a entrar hay que ejecutar el comando otra vez.
- `/max off ManzanitaSpace` lo desactiva manualmente.
- Requiere nivel de permiso 2 (operador).

Ars Nouveau y Mine and Slash reciben 999 de maná, regeneración y poder mágico. Iron's Spells recibe 999 de maná; sus atributos porcentuales usan 100 porque ése es el máximo real que admite el mod. Mine and Slash usa 300 de velocidad de casteo y 75 de reducción de enfriamiento, sus topes reales.

## InControl

La configuración sólo limita intentos nuevos de aparición natural cuando ya existe una cantidad alta de entidades por jugador. No elimina entidades existentes y no bloquea invocaciones, huevos, spawners, estructuras, comandos, crianza ni jefes.

Los topes son conservadores: 60 hostiles, 30 pasivos y 110 entidades totales por jugador para apariciones naturales. Si el servidor ya tiene otro `spawn.json`, combina estas reglas en vez de reemplazarlo sin revisar.
