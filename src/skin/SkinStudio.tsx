import { invoke } from '@tauri-apps/api/core'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import * as THREE from 'three'
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
import { buildMinecraftModel, defaultLayerVisibility, type ModelLayersVisibility, type ModelVariant, type SkinPartKey } from './editor/minecraftModel'
import { hexToRgba, hsvToRgb, rgbToHsv } from './editor/color'
import type { PaintContext, SelectionRect, ToolActionResult, ToolId } from './editor/types'
import { applyBrush } from './editor/tools/brushTool'
import { applyEraser } from './editor/tools/eraserTool'
import { applyEyedropper } from './editor/tools/eyedropperTool'
import { applyFill } from './editor/tools/fillTool'
import { buildSelection } from './editor/tools/rectSelectTool'

type SkinSummary = { id: string; name: string; updated_at: string }

type ThreeCtx = {
  scene: THREE.Scene
  camera: THREE.PerspectiveCamera
  renderer: THREE.WebGLRenderer
  controls: OrbitControls
  model: THREE.Group
  texture: THREE.CanvasTexture
  raycaster: THREE.Raycaster
  pointer: THREE.Vector2
  mount: HTMLDivElement
}

const TOOL_LABELS: Record<ToolId, string> = { brush: 'Pincel', eraser: 'Borrador', eyedropper: 'Cuentagotas', fill: 'Relleno', rect: 'Selector' }
const PARTS: Array<{ key: SkinPartKey; label: string }> = [
  { key: 'head', label: 'Cabeza' },
  { key: 'body', label: 'Torso' },
  { key: 'leftArm', label: 'Brazo Izq' },
  { key: 'rightArm', label: 'Brazo Der' },
  { key: 'leftLeg', label: 'Pierna Izq' },
  { key: 'rightLeg', label: 'Pierna Der' },
]

export function SkinStudio({ activePage, selectedAccountId, onNavigateEditor }: { activePage: 'Administradora de skins' | 'Editor de skins'; selectedAccountId: string; onNavigateEditor: () => void }) {
  const [skins, setSkins] = useState<SkinSummary[]>([])
  const [selectedSkinId, setSelectedSkinId] = useState('')
  const [tabs, setTabs] = useState<SkinSummary[]>([])
  const [activeTab, setActiveTab] = useState('')
  const [tool, setTool] = useState<ToolId>('brush')
  const [variant, setVariant] = useState<ModelVariant>('classic')
  const [color, setColor] = useState('#ff4d4f')
  const [hsv, setHsv] = useState({ h: 0, s: 1, v: 1 })
  const [brushSize, setBrushSize] = useState(2)
  const [hardness, setHardness] = useState(1)
  const [opacity, setOpacity] = useState(1)
  const [symmetry, setSymmetry] = useState(false)
  const [error, setError] = useState('')
  const [hasPendingChanges, setHasPendingChanges] = useState(false)
  const [colorHistory, setColorHistory] = useState<string[]>(['#ff4d4f'])
  const [customColors, setCustomColors] = useState<string[]>([])
  const [selection, setSelection] = useState<SelectionRect>(null)
  const [texHeight, setTexHeight] = useState<64 | 128>(64)
  const [layerVisibility, setLayerVisibility] = useState<ModelLayersVisibility>(() => defaultLayerVisibility())

  const threeRef = useRef<ThreeCtx | null>(null)
  const renderRafRef = useRef<number | null>(null)
  const texCanvasRef = useRef<HTMLCanvasElement | null>(null)
  const texCtxRef = useRef<CanvasRenderingContext2D | null>(null)
  const pixelBufferRef = useRef<ImageData | null>(null)
  const previewCanvasRef = useRef<HTMLCanvasElement | null>(null)
  const editorCanvasRef = useRef<HTMLCanvasElement | null>(null)
  const rectStartRef = useRef<{ x: number; y: number } | null>(null)

  const selectedSkin = useMemo(() => skins.find((item) => item.id === selectedSkinId), [skins, selectedSkinId])
  const activeSkin = useMemo(() => tabs.find((item) => item.id === activeTab), [tabs, activeTab])

  const setActiveColor = useCallback((next: string) => {
    setColor(next)
    const rgba = hexToRgba(next)
    setHsv(rgbToHsv(rgba.r, rgba.g, rgba.b))
    setColorHistory((prev) => [next, ...prev.filter((v) => v !== next)].slice(0, 10))
  }, [])

  const scheduleRender = useCallback(() => {
    if (renderRafRef.current || !threeRef.current) return
    renderRafRef.current = requestAnimationFrame(() => {
      renderRafRef.current = null
      const current = threeRef.current
      if (!current) return
      current.controls.update()
      current.renderer.render(current.scene, current.camera)
    })
  }, [])

  const updatePreview = useCallback(() => {
    const source = texCanvasRef.current
    if (!source) return
    const draw = (target: HTMLCanvasElement | null, size: number) => {
      if (!target) return
      const ctx = target.getContext('2d')
      if (!ctx) return
      target.width = size
      target.height = Math.round((size * texHeight) / 64)
      ctx.imageSmoothingEnabled = false
      ctx.clearRect(0, 0, size, Math.round((size * texHeight) / 64))
      ctx.drawImage(source, 0, 0, size, Math.round((size * texHeight) / 64))
      if (selection) {
        ctx.strokeStyle = '#ffffff'
        ctx.lineWidth = 2
        ctx.strokeRect((selection.x / 64) * size, (selection.y / texHeight) * Math.round((size * texHeight) / 64), (selection.width / 64) * size, (selection.height / texHeight) * Math.round((size * texHeight) / 64))
      }
    }
    draw(previewCanvasRef.current, 180)
    draw(editorCanvasRef.current, 360)
  }, [selection, texHeight])

  const syncTexture = useCallback(() => {
    const three = threeRef.current
    if (!three) return
    three.texture.needsUpdate = true
    scheduleRender()
    updatePreview()
  }, [scheduleRender, updatePreview])

  const loadSkins = useCallback(async () => {
    if (!selectedAccountId) return
    const data = await invoke<SkinSummary[]>('list_skins', { accountId: selectedAccountId })
    setSkins(data)
    setSelectedSkinId((prev) => prev || data[0]?.id || '')
  }, [selectedAccountId])

  useEffect(() => { void loadSkins().catch((err) => setError(String(err))) }, [loadSkins])

  useEffect(() => {
    const canvas = document.createElement('canvas')
    canvas.width = 64
    canvas.height = 64
    texCanvasRef.current = canvas
    const ctx = canvas.getContext('2d', { willReadFrequently: true })
    if (!ctx) return
    ctx.imageSmoothingEnabled = false
    texCtxRef.current = ctx
    pixelBufferRef.current = ctx.getImageData(0, 0, 64, 64)
  }, [])

  const replaceModel = useCallback((textureHeight: 64 | 128) => {
    const ctx = threeRef.current
    if (!ctx) return
    ctx.scene.remove(ctx.model)
    const model = buildMinecraftModel(ctx.texture, variant, textureHeight, layerVisibility)
    setTexHeight(textureHeight)
    ctx.scene.add(model)
    ctx.model = model
    scheduleRender()
  }, [scheduleRender, variant, layerVisibility])

  useEffect(() => {
    if (activePage !== 'Editor de skins') return
    const mount = document.getElementById('skin-three-root') as HTMLDivElement | null
    const texCanvas = texCanvasRef.current
    if (!mount || !texCanvas) return

    const scene = new THREE.Scene()
    scene.background = new THREE.Color('#0c111b')
    const camera = new THREE.PerspectiveCamera(45, mount.clientWidth / mount.clientHeight, 0.1, 260)
    camera.position.set(26, 20, 26)

    const renderer = new THREE.WebGLRenderer({ antialias: true, powerPreference: 'high-performance' })
    renderer.setSize(mount.clientWidth, mount.clientHeight)
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2))
    mount.innerHTML = ''
    mount.appendChild(renderer.domElement)

    scene.add(new THREE.AmbientLight(0xffffff, 0.7))
    const key = new THREE.DirectionalLight(0xffffff, 0.65)
    key.position.set(20, 28, 18)
    scene.add(key)
    const rim = new THREE.DirectionalLight(0x9db8ff, 0.35)
    rim.position.set(-16, 14, -20)
    scene.add(rim)

    const texture = new THREE.CanvasTexture(texCanvas)
    texture.magFilter = THREE.NearestFilter
    texture.minFilter = THREE.NearestFilter
    texture.generateMipmaps = false
    texture.colorSpace = THREE.SRGBColorSpace

    const model = buildMinecraftModel(texture, variant, texHeight, layerVisibility)
    scene.add(model)

    const controls = new OrbitControls(camera, renderer.domElement)
    controls.enableDamping = true
    controls.enablePan = true
    controls.minDistance = 7
    controls.maxDistance = 90
    controls.target.set(0, 10, 0)
    controls.mouseButtons.LEFT = THREE.MOUSE.ROTATE
    controls.mouseButtons.RIGHT = THREE.MOUSE.PAN
    controls.mouseButtons.MIDDLE = THREE.MOUSE.DOLLY
    controls.addEventListener('change', scheduleRender)

    const raycaster = new THREE.Raycaster()
    const pointer = new THREE.Vector2()

    threeRef.current = { scene, camera, renderer, controls, model, texture, raycaster, pointer, mount }

    const onResize = () => {
      const c = threeRef.current
      if (!c) return
      c.camera.aspect = c.mount.clientWidth / c.mount.clientHeight
      c.camera.updateProjectionMatrix()
      c.renderer.setSize(c.mount.clientWidth, c.mount.clientHeight)
      scheduleRender()
    }

    window.addEventListener('resize', onResize)
    scheduleRender()

    return () => {
      window.removeEventListener('resize', onResize)
      controls.dispose()
      renderer.dispose()
      if (renderRafRef.current) cancelAnimationFrame(renderRafRef.current)
      threeRef.current = null
    }
  }, [activePage, scheduleRender, texHeight, variant, layerVisibility])

  const paintContext = useCallback((): PaintContext | null => {
    const buffer = pixelBufferRef.current
    if (!buffer) return null
    return {
      width: 64,
      height: texHeight,
      selection,
      getPixel: (x, y) => {
        const i = (y * 64 + x) * 4
        return { r: buffer.data[i], g: buffer.data[i + 1], b: buffer.data[i + 2], a: buffer.data[i + 3] }
      },
      setPixel: (x, y, p) => {
        const i = (y * 64 + x) * 4
        buffer.data[i] = p.r
        buffer.data[i + 1] = p.g
        buffer.data[i + 2] = p.b
        buffer.data[i + 3] = p.a
      },
    }
  }, [selection, texHeight])

  const flushPixels = useCallback(() => {
    const ctx = texCtxRef.current
    const buffer = pixelBufferRef.current
    if (!ctx || !buffer) return
    ctx.putImageData(buffer, 0, 0)
    setHasPendingChanges(true)
    syncTexture()
  }, [syncTexture])

  const executeTool = useCallback((x: number, y: number) => {
    const pctx = paintContext()
    if (!pctx) return
    const colorRgba = hexToRgba(color, opacity)
    let result: ToolActionResult = { changed: false }

    if (tool === 'brush') result = applyBrush(pctx, { x, y }, colorRgba, { size: brushSize, hardness, opacity, symmetry })
    if (tool === 'eraser') result = applyEraser(pctx, { x, y }, { size: brushSize, hardness, opacity, symmetry })
    if (tool === 'eyedropper') result = applyEyedropper(pctx, { x, y })
    if (tool === 'fill') result = applyFill(pctx, { x, y }, colorRgba)

    if (result.pickedColor) setActiveColor(result.pickedColor)
    if (result.changed) flushPixels()
  }, [paintContext, color, opacity, tool, brushSize, hardness, symmetry, flushPixels, setActiveColor])

  const loadSkinToEditor = async (skinId: string) => {
    if (!selectedAccountId) return
    const bytes = await invoke<number[]>('load_skin_binary', { accountId: selectedAccountId, skinId })
    const blob = new Blob([new Uint8Array(bytes)], { type: 'image/png' })
    const bitmap = await createImageBitmap(blob)
    const ctx = texCtxRef.current
    if (!ctx) return
    const h = bitmap.height === 128 ? 128 : 64
    setTexHeight(h)
    ctx.canvas.height = h
    ctx.clearRect(0, 0, 64, h)
    ctx.drawImage(bitmap, 0, 0, 64, h)
    pixelBufferRef.current = ctx.getImageData(0, 0, 64, h)
    setSelection(null)
    setLayerVisibility(defaultLayerVisibility())
    replaceModel(h)
    setHasPendingChanges(false)
    syncTexture()
  }

  const saveSkin = async () => {
    if (!activeSkin || !selectedAccountId || !texCanvasRef.current) return
    const blob = await new Promise<Blob>((resolve, reject) => texCanvasRef.current?.toBlob((value) => value ? resolve(value) : reject(new Error('No se pudo exportar PNG')), 'image/png'))
    const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()))
    await invoke('save_skin_binary', { accountId: selectedAccountId, skinId: activeSkin.id, bytes })
    setHasPendingChanges(false)
    await loadSkins()
  }

  const paintFromTextureEvent = (event: React.PointerEvent<HTMLCanvasElement>) => {
    const rect = event.currentTarget.getBoundingClientRect()
    const x = Math.floor(((event.clientX - rect.left) / rect.width) * 64)
    const y = Math.floor(((event.clientY - rect.top) / rect.height) * texHeight)
    if (x < 0 || y < 0 || x >= 64 || y >= texHeight) return

    if (tool === 'rect') {
      if (!rectStartRef.current) rectStartRef.current = { x, y }
      return
    }

    executeTool(x, y)
  }

  const paintFromModelEvent = (event: React.PointerEvent<HTMLDivElement>) => {
    const three = threeRef.current
    if (!three) return
    const rect = event.currentTarget.getBoundingClientRect()
    three.pointer.set(((event.clientX - rect.left) / rect.width) * 2 - 1, -((event.clientY - rect.top) / rect.height) * 2 + 1)
    three.raycaster.setFromCamera(three.pointer, three.camera)
    const intersects = three.raycaster.intersectObjects(three.model.children, true)
    const uv = intersects[0]?.uv
    if (!uv) return
    const x = Math.min(63, Math.max(0, Math.floor(uv.x * 64)))
    const y = Math.min(texHeight - 1, Math.max(0, Math.floor((1 - uv.y) * texHeight)))
    executeTool(x, y)
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
            <label className="button-like">Importar PNG
              <input
                type="file"
                accept="image/png"
                hidden
                onChange={async (e) => {
                  const file = e.target.files?.[0]
                  if (!file || !selectedAccountId) return
                  const bytes = Array.from(new Uint8Array(await file.arrayBuffer()))
                  await invoke('import_skin', { accountId: selectedAccountId, name: file.name.replace('.png', ''), bytes })
                  await loadSkins()
                }}
              />
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
    <main className="skin-editor-page pro-studio">
      <header className="skin-tabs-bar browser-tabs">
        {tabs.length === 0 && <span className="tab-empty">No hay skins abiertas.</span>}
        {tabs.map((tab) => (
          <button key={tab.id} className={activeTab === tab.id ? 'active' : ''} onClick={() => { setActiveTab(tab.id); void loadSkinToEditor(tab.id) }}>
            {tab.name}
          </button>
        ))}
      </header>

      <header className="skin-tools-bar">
        {(Object.keys(TOOL_LABELS) as ToolId[]).map((id) => (
          <button key={id} className={tool === id ? 'active' : ''} onClick={() => setTool(id)}>{TOOL_LABELS[id]}</button>
        ))}
        <label>Tamaño <input type="range" min={1} max={16} value={brushSize} onChange={(e) => setBrushSize(Number(e.target.value))} /></label>
        <label>Dureza <input type="range" min={0.1} max={1} step={0.1} value={hardness} onChange={(e) => setHardness(Number(e.target.value))} /></label>
        <label>Opacidad <input type="range" min={0.1} max={1} step={0.05} value={opacity} onChange={(e) => setOpacity(Number(e.target.value))} /></label>
        <label><input type="checkbox" checked={symmetry} onChange={(e) => setSymmetry(e.target.checked)} /> Simetría</label>
        <select value={variant} onChange={(e) => setVariant(e.target.value as ModelVariant)}>
          <option value="classic">Classic</option>
          <option value="slim">Slim</option>
        </select>
        <button className="primary" disabled={!activeSkin || !hasPendingChanges} onClick={() => void saveSkin()}>Guardar</button>
      </header>

      <section className="skin-editor-workspace pro compact">
        <aside className="editor-left-sidebar">
          <h3>Textura</h3>
          <canvas ref={previewCanvasRef} className="texture-preview" />
          <h3>Canvas pixel</h3>
          <canvas
            ref={editorCanvasRef}
            width={64}
            height={texHeight}
            className="paint-canvas"
            onPointerDown={paintFromTextureEvent}
            onPointerMove={(e) => e.buttons === 1 && paintFromTextureEvent(e)}
            onPointerUp={(e) => {
              if (tool !== 'rect' || !rectStartRef.current) return
              const rect = e.currentTarget.getBoundingClientRect()
              const x = Math.floor(((e.clientX - rect.left) / rect.width) * 64)
              const y = Math.floor(((e.clientY - rect.top) / rect.height) * texHeight)
              setSelection(buildSelection(rectStartRef.current, { x, y }))
              rectStartRef.current = null
              updatePreview()
            }}
          />
        </aside>

        <section className="skin-editor-canvas">
          <div id="skin-three-root" className="three-preview" onPointerDown={paintFromModelEvent} onPointerMove={(e) => e.buttons === 1 && paintFromModelEvent(e)} />
        </section>

        <aside className="editor-right-sidebar">
          <div className="right-panel-content">
            <h3>Paleta</h3>
            <label>HEX <input value={color} onChange={(e) => setActiveColor(e.target.value)} /></label>
            <div className="hsv-grid">
              <label>H <input type="range" min={0} max={360} value={hsv.h} onChange={(e) => { const next = { ...hsv, h: Number(e.target.value) }; setHsv(next); const rgb = hsvToRgb(next.h, next.s, next.v); setActiveColor(`#${[rgb.r, rgb.g, rgb.b].map((n) => n.toString(16).padStart(2, '0')).join('')}`) }} /></label>
              <label>S <input type="range" min={0} max={1} step={0.01} value={hsv.s} onChange={(e) => { const next = { ...hsv, s: Number(e.target.value) }; setHsv(next); const rgb = hsvToRgb(next.h, next.s, next.v); setActiveColor(`#${[rgb.r, rgb.g, rgb.b].map((n) => n.toString(16).padStart(2, '0')).join('')}`) }} /></label>
              <label>V <input type="range" min={0} max={1} step={0.01} value={hsv.v} onChange={(e) => { const next = { ...hsv, v: Number(e.target.value) }; setHsv(next); const rgb = hsvToRgb(next.h, next.s, next.v); setActiveColor(`#${[rgb.r, rgb.g, rgb.b].map((n) => n.toString(16).padStart(2, '0')).join('')}`) }} /></label>
            </div>
            <h4>Historial</h4>
            <div className="swatches">{colorHistory.map((v) => <button key={v} style={{ background: v }} onClick={() => setActiveColor(v)} />)}</div>
            <h4>Personalizados</h4>
            <div className="swatches">{customColors.map((v) => <button key={v} style={{ background: v }} onClick={() => setActiveColor(v)} />)}<button onClick={() => setCustomColors((prev) => [color, ...prev.filter((v) => v !== color)].slice(0, 10))}>+</button></div>

            <h3>Capas</h3>
            <ul className="layer-list layer-grid">
              {PARTS.map((part) => (
                <li key={part.key}>
                  <strong>{part.label}</strong>
                  <div className="layer-row-actions">
                    <button className={layerVisibility[part.key].base ? 'active' : ''} onClick={() => setLayerVisibility((prev) => ({ ...prev, [part.key]: { ...prev[part.key], base: !prev[part.key].base } }))}>Base</button>
                    <button className={layerVisibility[part.key].overlay ? 'active' : ''} onClick={() => setLayerVisibility((prev) => ({ ...prev, [part.key]: { ...prev[part.key], overlay: !prev[part.key].overlay } }))}>Overlay</button>
                  </div>
                </li>
              ))}
            </ul>
          </div>
        </aside>
      </section>
    </main>
  )
}
