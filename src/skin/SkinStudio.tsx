import { invoke } from '@tauri-apps/api/core'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { create } from 'zustand'
import * as THREE from 'three'

type SkinSummary = { id: string; name: string; updated_at: string }
type Tool = 'Pincel' | 'Bote' | 'Borrador' | 'Gotero' | 'Figuras' | 'Gradiante' | 'Seleccion' | 'Mover'
type RightMode = 'Paint' | 'Pose'

type StudioState = {
  tool: Tool
  color: string
  brushSize: number
  opacity: number
  rightMode: RightMode
  setTool: (tool: Tool) => void
  setColor: (color: string) => void
  setBrush: (size: number, opacity: number) => void
  setRightMode: (mode: RightMode) => void
}

const useStudioStore = create<StudioState>((set) => ({
  tool: 'Pincel',
  color: '#ff4d4f',
  brushSize: 2,
  opacity: 1,
  rightMode: 'Paint',
  setTool: (tool) => set({ tool }),
  setColor: (color) => set({ color }),
  setBrush: (brushSize, opacity) => set({ brushSize, opacity }),
  setRightMode: (rightMode) => set({ rightMode }),
}))

const layerItems = ['Cabeza', 'Capa Cabeza', 'Torso', 'Capa Torso', 'Brazo Izq', 'Capa Brazo Izq', 'Brazo Der', 'Capa Brazo Der', 'Pierna Izq', 'Capa Pierna Izq', 'Pierna Der', 'Capa Pierna Der']

export function SkinStudio({ activePage, selectedAccountId, onNavigateEditor }: { activePage: 'Administradora de skins' | 'Editor de skins'; selectedAccountId: string; onNavigateEditor: () => void }) {
  const [skins, setSkins] = useState<SkinSummary[]>([])
  const [selectedSkinId, setSelectedSkinId] = useState('')
  const [tabs, setTabs] = useState<SkinSummary[]>([])
  const [activeTab, setActiveTab] = useState('')
  const [error, setError] = useState('')
  const [textureBitmap, setTextureBitmap] = useState<ImageBitmap | null>(null)
  const [hasPendingChanges, setHasPendingChanges] = useState(false)
  const editorCanvasRef = useRef<HTMLCanvasElement | null>(null)
  const workerRef = useRef<Worker | null>(null)
  const textureRef = useRef<THREE.CanvasTexture | null>(null)
  const previewCanvasRef = useRef<HTMLCanvasElement | null>(null)

  const { tool, color, brushSize, opacity, rightMode, setTool, setColor, setBrush, setRightMode } = useStudioStore()

  const selectedSkin = useMemo(() => skins.find((item) => item.id === selectedSkinId), [skins, selectedSkinId])
  const activeSkin = useMemo(() => tabs.find((item) => item.id === activeTab), [tabs, activeTab])

  const loadSkins = useCallback(async () => {
    if (!selectedAccountId) return
    try {
      const data = await invoke<SkinSummary[]>('list_skins', { accountId: selectedAccountId })
      setSkins(data)
      setSelectedSkinId((prev) => prev || data[0]?.id || '')
    } catch (err) {
      setError(String(err))
    }
  }, [selectedAccountId])

  // eslint-disable-next-line react-hooks/set-state-in-effect
  useEffect(() => { void loadSkins() }, [loadSkins])

  useEffect(() => {
    const worker = new Worker(new URL('./paintWorker.ts', import.meta.url), { type: 'module' })
    const canvas = editorCanvasRef.current
    if (!canvas) return
    const offscreen = canvas.transferControlToOffscreen()
    worker.postMessage({ type: 'init', payload: { offscreen, width: 64, height: 64 } }, [offscreen])
    worker.onmessage = (event: MessageEvent<{ type: string; bitmap?: ImageBitmap; color?: string }>) => {
      if (event.data.type === 'bitmap' && event.data.bitmap) {
        setTextureBitmap(event.data.bitmap)
        setHasPendingChanges(true)
      }
      if (event.data.type === 'pickedColor' && event.data.color) {
        setColor(event.data.color)
      }
    }
    workerRef.current = worker
    return () => worker.terminate()
  }, [setColor])

  useEffect(() => {
    workerRef.current?.postMessage({ type: 'settings', payload: { tool, color, brushSize, opacity } })
  }, [tool, color, brushSize, opacity])

  useEffect(() => {
    if (!textureBitmap) return
    const canvas = previewCanvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return
    canvas.width = 256
    canvas.height = 256
    ctx.imageSmoothingEnabled = false
    ctx.clearRect(0, 0, 256, 256)
    ctx.drawImage(textureBitmap, 0, 0, 256, 256)
  }, [textureBitmap])

  useEffect(() => {
    if (activePage !== 'Editor de skins') return
    const mount = document.getElementById('skin-three-root')
    if (!mount) return

    const scene = new THREE.Scene()
    scene.background = new THREE.Color('#111827')
    const camera = new THREE.PerspectiveCamera(40, mount.clientWidth / mount.clientHeight, 0.1, 100)
    camera.position.set(0, 1.2, 4)

    const renderer = new THREE.WebGLRenderer({ antialias: false, powerPreference: 'high-performance', precision: 'mediump' })
    renderer.setPixelRatio(1)
    renderer.shadowMap.enabled = false
    renderer.setSize(mount.clientWidth, mount.clientHeight)
    mount.innerHTML = ''
    mount.appendChild(renderer.domElement)

    const textureCanvas = document.createElement('canvas')
    textureCanvas.width = 64
    textureCanvas.height = 64
    const tctx = textureCanvas.getContext('2d')
    if (!tctx) return
    tctx.imageSmoothingEnabled = false

    const texture = new THREE.CanvasTexture(textureCanvas)
    texture.magFilter = THREE.NearestFilter
    texture.minFilter = THREE.NearestFilter
    textureRef.current = texture

    const material = new THREE.MeshBasicMaterial({ map: texture })
    const mesh = new THREE.Mesh(new THREE.BoxGeometry(1.4, 2, 0.8), material)
    scene.add(mesh)

    const rimMaterial = new THREE.ShaderMaterial({
      uniforms: { fresnelPower: { value: 2.2 }, rimColor: { value: new THREE.Color('#67e8f9') } },
      vertexShader: `varying vec3 vNormal; varying vec3 vView; void main(){ vec4 world = modelMatrix * vec4(position,1.0); vNormal = normalize(mat3(modelMatrix) * normal); vView = normalize(cameraPosition - world.xyz); gl_Position = projectionMatrix * viewMatrix * world; }`,
      fragmentShader: `uniform float fresnelPower; uniform vec3 rimColor; varying vec3 vNormal; varying vec3 vView; void main(){ float f = pow(1.0 - max(dot(normalize(vNormal), normalize(vView)), 0.0), fresnelPower); gl_FragColor = vec4(rimColor, f * 0.45); }`,
      transparent: true,
    })
    const rim = new THREE.Mesh(new THREE.BoxGeometry(1.42, 2.02, 0.82), rimMaterial)
    scene.add(rim)

    const animate = () => {
      mesh.rotation.y += 0.006
      rim.rotation.y = mesh.rotation.y
      renderer.render(scene, camera)
      requestAnimationFrame(animate)
    }
    animate()

    const onResize = () => {
      camera.aspect = mount.clientWidth / mount.clientHeight
      camera.updateProjectionMatrix()
      renderer.setSize(mount.clientWidth, mount.clientHeight)
    }
    window.addEventListener('resize', onResize)

    return () => {
      window.removeEventListener('resize', onResize)
      renderer.dispose()
    }
  }, [activePage])

  useEffect(() => {
    if (!textureBitmap || !textureRef.current) return
    const canvas = textureRef.current.image as HTMLCanvasElement
    const ctx = canvas.getContext('2d')
    if (!ctx) return
    ctx.clearRect(0, 0, 64, 64)
    ctx.drawImage(textureBitmap, 0, 0, 64, 64)
    textureRef.current.needsUpdate = true
  }, [textureBitmap])

  const loadSkinToEditor = async (skinId: string) => {
    if (!selectedAccountId) return
    const bytes = await invoke<number[]>('load_skin_binary', { accountId: selectedAccountId, skinId })
    const blob = new Blob([new Uint8Array(bytes)], { type: 'image/png' })
    const bitmap = await createImageBitmap(blob)
    workerRef.current?.postMessage({ type: 'loadImageBitmap', payload: { bitmap } }, [bitmap])
    setHasPendingChanges(false)
  }

  const saveSkin = async () => {
    if (!activeSkin || !selectedAccountId || !workerRef.current) return
    const w = workerRef.current
    const data = await new Promise<number[]>((resolve) => {
      const listener = (event: MessageEvent<{ type: string; bytes?: number[] }>) => {
        if (event.data.type === 'exported' && event.data.bytes) {
          w.removeEventListener('message', listener as EventListener)
          resolve(event.data.bytes)
        }
      }
      w.addEventListener('message', listener as EventListener)
      w.postMessage({ type: 'exportPng' })
    })
    await invoke('save_skin_binary', { accountId: selectedAccountId, skinId: activeSkin.id, bytes: data })
    await loadSkins()
    setHasPendingChanges(false)
  }

  if (activePage === 'Administradora de skins') {
    return (
      <main className="content content-padded">
        <h1 className="page-title">Administradora de skins</h1>
        <section className="skins-manager-layout">
          <section className="skins-catalog-panel">
            <header className="panel-header"><h2>Catálogo real</h2></header>
            {error && <p className="status-text error">{error}</p>}
            <div className="skins-grid">
              {skins.length === 0 && <article className="instance-card placeholder">Sin skins guardadas para esta cuenta.</article>}
              {skins.map((skin) => (
                <article key={skin.id} className={`instance-card clickable ${selectedSkinId === skin.id ? 'active' : ''}`} onClick={() => setSelectedSkinId(skin.id)}>
                  <strong>{skin.name}</strong>
                  <small>Actualizada: {skin.updated_at}</small>
                </article>
              ))}
            </div>
          </section>
          <aside className="account-manager-panel compact">
            <label className="button-like">
              Importar PNG
              <input type="file" accept="image/png" hidden onChange={async (e) => {
                const file = e.target.files?.[0]
                if (!file || !selectedAccountId) return
                const bytes = Array.from(new Uint8Array(await file.arrayBuffer()))
                await invoke('import_skin', { accountId: selectedAccountId, name: file.name.replace('.png', ''), bytes })
                await loadSkins()
              }} />
            </label>
            <button disabled={!selectedSkin} onClick={async () => {
              if (!selectedSkin) return
              setTabs((prev) => prev.some((tab) => tab.id === selectedSkin.id) ? prev : [...prev, selectedSkin])
              setActiveTab(selectedSkin.id)
              await loadSkinToEditor(selectedSkin.id)
              onNavigateEditor()
            }}>Editar</button>
            <button disabled={!selectedSkin} onClick={async () => {
              if (!selectedSkin || !selectedAccountId) return
              await invoke('delete_skin', { accountId: selectedAccountId, skinId: selectedSkin.id })
              await loadSkins()
            }}>Eliminar</button>
          </aside>
        </section>
      </main>
    )
  }

  return (
    <main className="skin-editor-page">
      <header className="skin-tabs-bar browser-tabs">
        {tabs.length === 0 && <span className="tab-empty">No hay skins abiertas.</span>}
        {tabs.map((tab) => <button key={tab.id} className={activeTab === tab.id ? 'active' : ''} onClick={() => { setActiveTab(tab.id); void loadSkinToEditor(tab.id) }}>{tab.name}</button>)}
      </header>

      <header className="skin-tools-bar">
        {(['Pincel', 'Bote', 'Borrador', 'Gotero', 'Figuras', 'Gradiante', 'Seleccion', 'Mover'] as Tool[]).map((item) => (
          <button key={item} className={tool === item ? 'active' : ''} onClick={() => setTool(item)}>{item}</button>
        ))}
        <label>Brush Plus Tamaño <input type="range" min={1} max={12} value={brushSize} onChange={(e) => setBrush(Number(e.target.value), opacity)} /></label>
        <label>Opacidad <input type="range" min={0.1} max={1} step={0.05} value={opacity} onChange={(e) => setBrush(brushSize, Number(e.target.value))} /></label>
        <button>Suavizado</button><button>Espejo</button><button>Cuadriculado</button>
        <button className="primary" disabled={!activeSkin || !hasPendingChanges} onClick={() => void saveSkin()}>Guardar</button>
      </header>

      <section className="skin-editor-workspace pro">
        <aside className="editor-left-sidebar">
          <h3>Texturas</h3>
          <canvas ref={previewCanvasRef} className="texture-preview" />
          <h3>Paint (PNG completo)</h3>
          <canvas
            ref={editorCanvasRef}
            width={64}
            height={64}
            className="paint-canvas"
            onPointerDown={(e) => {
              const rect = e.currentTarget.getBoundingClientRect()
              const x = Math.floor(((e.clientX - rect.left) / rect.width) * 64)
              const y = Math.floor(((e.clientY - rect.top) / rect.height) * 64)
              workerRef.current?.postMessage({ type: 'paint', payload: { x, y } })
            }}
            onPointerMove={(e) => {
              if (e.buttons !== 1) return
              const rect = e.currentTarget.getBoundingClientRect()
              const x = Math.floor(((e.clientX - rect.left) / rect.width) * 64)
              const y = Math.floor(((e.clientY - rect.top) / rect.height) * 64)
              workerRef.current?.postMessage({ type: 'paint', payload: { x, y } })
            }}
          />
        </aside>

        <section className="skin-editor-canvas"><div id="skin-three-root" className="three-preview" /></section>

        <aside className="editor-right-sidebar">
          <div className="right-mode-switch"><button className={rightMode === 'Paint' ? 'active' : ''} onClick={() => setRightMode('Paint')}>Paint</button><button className={rightMode === 'Pose' ? 'active' : ''} onClick={() => setRightMode('Pose')}>Pose</button></div>
          {rightMode === 'Paint' ? (
            <div className="right-panel-content">
              <h3>Paleta profesional</h3>
              <input type="color" value={color} onChange={(e) => setColor(e.target.value)} />
              <div className="swatches">{['#ff4d4f', '#faad14', '#36cfc9', '#597ef7', '#eb2f96', '#f5f5f5'].map((v) => <button key={v} style={{ background: v }} onClick={() => setColor(v)} />)}</div>
              <h3>Capas</h3>
              <ul className="layer-list">{layerItems.map((layer) => <li key={layer}>{layer}</li>)}</ul>
            </div>
          ) : (
            <div className="right-panel-content">
              <h3>Capas</h3>
              <ul className="layer-list">{layerItems.map((layer) => <li key={layer}>{layer}</li>)}</ul>
              <h3>Sistema de poses</h3>
              <div className="pose-placeholder">Base de poses preparada (pendiente de presets avanzados)</div>
            </div>
          )}
        </aside>
      </section>
    </main>
  )
}
