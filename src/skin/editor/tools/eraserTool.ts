import type { BrushSettings, PaintContext, PaintPoint, ToolActionResult } from '../types'

export const applyEraser = (ctx: PaintContext, point: PaintPoint, settings: BrushSettings): ToolActionResult => {
  const radius = Math.max(0.5, settings.size / 2)
  let changed = false
  for (let y = Math.floor(point.y - radius); y <= Math.ceil(point.y + radius); y += 1) {
    for (let x = Math.floor(point.x - radius); x <= Math.ceil(point.x + radius); x += 1) {
      if (x < 0 || y < 0 || x >= ctx.width || y >= ctx.height) continue
      if (Math.hypot(x - point.x, y - point.y) > radius) continue
      ctx.setPixel(x, y, { r: 0, g: 0, b: 0, a: 0 })
      changed = true
      if (settings.symmetry) {
        const mx = ctx.width - 1 - x
        ctx.setPixel(mx, y, { r: 0, g: 0, b: 0, a: 0 })
      }
    }
  }
  return { changed }
}
