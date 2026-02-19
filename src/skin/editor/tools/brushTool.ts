import { blend } from '../color'
import type { BrushSettings, PaintContext, PaintPoint, RGBA, ToolActionResult } from '../types'

const inSelection = (x: number, y: number, selection: PaintContext['selection']) => !selection || (x >= selection.x && y >= selection.y && x < selection.x + selection.width && y < selection.y + selection.height)

export const applyBrush = (ctx: PaintContext, point: PaintPoint, color: RGBA, settings: BrushSettings): ToolActionResult => {
  const radius = Math.max(0.5, settings.size / 2)
  let changed = false
  for (let y = Math.floor(point.y - radius); y <= Math.ceil(point.y + radius); y += 1) {
    for (let x = Math.floor(point.x - radius); x <= Math.ceil(point.x + radius); x += 1) {
      if (x < 0 || y < 0 || x >= ctx.width || y >= ctx.height || !inSelection(x, y, ctx.selection)) continue
      const dist = Math.hypot(x - point.x, y - point.y)
      if (dist > radius) continue
      const falloff = settings.hardness >= 1 ? 1 : Math.max(0, 1 - (dist / radius) * (1 - settings.hardness))
      const top = { ...color, a: Math.round(color.a * falloff) }
      const current = ctx.getPixel(x, y)
      ctx.setPixel(x, y, blend(current, top))
      changed = true
      if (settings.symmetry) {
        const mx = ctx.width - 1 - x
        if (mx !== x) {
          const mirrorCurrent = ctx.getPixel(mx, y)
          ctx.setPixel(mx, y, blend(mirrorCurrent, top))
        }
      }
    }
  }
  return { changed }
}
