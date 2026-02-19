import { rgbaToHex } from '../color'
import type { PaintContext, PaintPoint, ToolActionResult } from '../types'

export const applyEyedropper = (ctx: PaintContext, point: PaintPoint): ToolActionResult => {
  const x = Math.max(0, Math.min(ctx.width - 1, Math.round(point.x)))
  const y = Math.max(0, Math.min(ctx.height - 1, Math.round(point.y)))
  return { changed: false, pickedColor: rgbaToHex(ctx.getPixel(x, y)) }
}
