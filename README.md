# INTERFACE

**INTERFACE** es un launcher de escritorio multiplataforma con arquitectura híbrida **Tauri + React + Rust**, diseñado para gestionar instancias, importar perfiles existentes y ejecutar entornos de juego con enfoque en estabilidad, rendimiento y seguridad.

## Qué es INTERFACE

INTERFACE centraliza en una sola aplicación:

- Gestión de instancias locales.
- Importación y migración de instancias desde carpetas externas.
- Descarga/verificación de archivos necesarios.
- Configuración de Java, memoria y parámetros de ejecución.
- Herramientas visuales adicionales (por ejemplo, editor/estudio de skins).

## Funciones principales

- **Explorador de instancias** con flujo de administración y apertura de carpetas.
- **Sistema de importación** con escaneo, detección y ejecución por lotes.
- **Instalación/gestión de loaders** y componentes de runtime.
- **Gestión de versiones y servicios** de autenticación, ajustes y lanzamiento.
- **Descargador con integridad** (hash/checksum) y utilidades de caché.
- **Bloqueo de archivos y operaciones seguras de FS** para evitar corrupción de datos.
- **UI moderna** en React con estado global y experiencia de escritorio vía Tauri.

## Lenguajes y tecnologías usadas

### Frontend

- **TypeScript**
- **React 19**
- **Vite**
- **Zustand**
- **Three.js** (módulos visuales)

### Backend / Core

- **Rust (edition 2021)**
- **Tauri 2**
- **Tokio** (concurrencia async)
- **Reqwest + Rustls** (HTTP/TLS)
- **Serde / Serde JSON**
- **SHA-1 / SHA-2** para verificación de integridad

## Métodos y arquitectura implementados

- **Arquitectura por capas**:
  - `commands` (API invocable desde UI)
  - `app` (servicios de dominio de aplicación)
  - `domain` (modelos y lógica de negocio)
  - `infrastructure` (filesystem, descarga, caché, checksum)
  - `runtime` (procesos, memoria, entorno)
- **Validación y saneamiento de rutas/nombres** para prevenir rutas inválidas.
- **Copias controladas por límites** durante importaciones masivas.
- **Persistencia explícita de metadata** por instancia para trazabilidad.
- **Emisión de eventos de progreso** entre backend y frontend para seguimiento en tiempo real.

## Rendimiento y seguridad (comparativa general)

Sin mencionar marcas concretas, INTERFACE implementa mejoras que, en términos generales, suelen superar a launchers tradicionales en:

### Rendimiento

- **Core nativo en Rust**: menor sobrecarga de CPU/RAM frente a soluciones puramente interpretadas.
- **Concurrencia eficiente (Tokio)** para I/O y descargas paralelas.
- **Caché y cola de descargas** para reducir trabajo repetido.
- **Gestión segmentada de archivos** en importaciones (evita cargas innecesarias).

### Seguridad e integridad

- **Verificación por checksum/hash** de archivos críticos descargados.
- **Uso de TLS moderno (Rustls)** en comunicaciones HTTP seguras.
- **Saneamiento de nombres y rutas** para mitigar path traversal y errores de escritura.
- **Operaciones de filesystem con lock y validaciones** para reducir corrupción o estados inconsistentes.

## Instalación (desarrollo)

### Requisitos

- **Node.js** (recomendado LTS)
- **npm**
- **Rust toolchain** (`rustup`, `cargo`)
- Dependencias de sistema requeridas por **Tauri 2** según tu SO

### Pasos

```bash
# 1) Clonar repositorio
git clone <URL_DEL_REPOSITORIO>
cd Interface-2

# 2) Instalar dependencias de frontend
npm install

# 3) Ejecutar en modo desarrollo
npm run tauri dev
```

## Compilación

### Build del frontend

```bash
npm run build
```

### Build de aplicación de escritorio (Tauri)

```bash
npm run tauri build
```

Los artefactos finales se generan mediante el pipeline de Tauri en `src-tauri` según la plataforma objetivo.

## Estructura base del proyecto

```text
src/                  # Frontend React/TypeScript
src-tauri/src/        # Backend Rust (commands, app, domain, infrastructure, runtime)
src-tauri/tauri.conf.json
```

## Estado del proyecto

INTERFACE está orientado a evolución continua. Se recomienda mantener dependencias actualizadas y validar cambios con lint/build antes de publicar.

## Actualizaciones automáticas (canales estable/beta)

- El updater de Tauri está configurado para consumir un manifiesto fijo en GitHub Pages:
  - `https://manzanitaspice.github.io/Interface-2/updates/stable.json`
- Se incluyen dos workflows de release:
  - `release-stable.yml`: tags `vX.Y.Z` publican release estable y actualizan `updates/stable.json`.
  - `release-beta.yml`: tags `vX.Y.Z-beta.N` publican prerelease y actualizan `updates/beta.json`.
- Los manifiestos de updater deben apuntar a **un solo tipo de bundle firmado**. En este repo se estandarizó `*.msi.zip` + su firma `*.msi.zip.sig`.
  - **No** mezclar NSIS y MSI en el feed del mismo canal.
  - **No** usar el instalador crudo (`*.exe` / `*.msi`) en `platforms.windows-x86_64.url`.
- Secrets requeridos en GitHub Actions:
  - `TAURI_SIGNING_PRIVATE_KEY`
  - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

Checklist rápido si no detecta updates:
1. `src-tauri/tauri.conf.json` tiene `plugins.updater.pubkey` y endpoint correctos.
2. La release tiene artefacto `*.msi.zip` **y** su archivo `.sig`.
3. `updates/stable.json` o `updates/beta.json` en `gh-pages` apunta a ese `.zip` y firma real.
4. El `version` del manifiesto es mayor que la versión actual (`getVersion()`) y respeta semver.

Esto evita depender de `releases/latest` y separa claramente los canales de actualización.
