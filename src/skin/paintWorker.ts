/// <reference lib="webworker" />

const worker = self as DedicatedWorkerGlobalScope
let canvas: OffscreenCanvas | null = null
let ctx: OffscreenCanvasRenderingContext2D | null = null
let color = '#ff4d4f'
let tool = 'Pincel'
let brushSize = 2
let opacity = 1

const emitBitmap = async () => {
  if (!canvas) return
  const bitmap = canvas.transferToImageBitmap()
  worker.postMessage({ type: 'bitmap', bitmap }, [bitmap])
}

worker.onmessage = async (event: MessageEvent) => {
  const { type, payload } = event.data

  if (type === 'init') {
    canvas = payload.offscreen as OffscreenCanvas
    canvas.width = payload.width
    canvas.height = payload.height
    ctx = canvas.getContext('2d', { willReadFrequently: true })
    if (!ctx) return
    ctx.imageSmoothingEnabled = false
    ctx.fillStyle = '#00000000'
    ctx.fillRect(0, 0, canvas.width, canvas.height)
    await emitBitmap()
    return
  }

  if (!ctx || !canvas) return

  if (type === 'loadImageBitmap') {
    const bitmap = payload.bitmap as ImageBitmap
    ctx.clearRect(0, 0, canvas.width, canvas.height)
    ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height)
    await emitBitmap()
    return
  }

  if (type === 'settings') {
    color = payload.color ?? color
    tool = payload.tool ?? tool
    brushSize = payload.brushSize ?? brushSize
    opacity = payload.opacity ?? opacity
    return
  }

  if (type === 'paint') {
    const { x, y } = payload
    if (tool === 'Gotero') {
      const p = ctx.getImageData(x, y, 1, 1).data
      worker.postMessage({ type: 'pickedColor', color: `#${[p[0], p[1], p[2]].map((v) => v.toString(16).padStart(2, '0')).join('')}` })
      return
    }

    ctx.globalAlpha = opacity
    if (tool === 'Borrador') {
      ctx.clearRect(x - brushSize / 2, y - brushSize / 2, brushSize, brushSize)
    } else if (tool === 'Bote') {
      ctx.fillStyle = color
      ctx.fillRect(0, 0, canvas.width, canvas.height)
    } else {
      ctx.fillStyle = color
      ctx.fillRect(x - brushSize / 2, y - brushSize / 2, brushSize, brushSize)
    }
    ctx.globalAlpha = 1
    await emitBitmap()
    return
  }

  if (type === 'exportPng') {
    const blob = await canvas.convertToBlob({ type: 'image/png' })
    const buffer = await blob.arrayBuffer()
    worker.postMessage({ type: 'exported', bytes: Array.from(new Uint8Array(buffer)) })
  }
}

export {}
