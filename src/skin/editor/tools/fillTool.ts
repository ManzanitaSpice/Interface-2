import { blend } from '../color'
import type { PaintContext, PaintPoint, RGBA, ToolActionResult } from '../types'

const same = (a: RGBA, b: RGBA) => a.r === b.r && a.g === b.g && a.b === b.b && a.a === b.a

export const applyFill = (ctx: PaintContext, point: PaintPoint, color: RGBA): ToolActionResult => {
  const sx = Math.max(0, Math.min(ctx.width - 1, Math.round(point.x)))
  const sy = Math.max(0, Math.min(ctx.height - 1, Math.round(point.y)))
  const source = ctx.getPixel(sx, sy)
  const target = blend(source, color)
  if (same(source, target)) return { changed: false }

  const q: Array<[number, number]> = [[sx, sy]]
  const visited = new Uint8Array(ctx.width * ctx.height)
  const idx = (x: number, y: number) => y * ctx.width + x
  let changed = false

  while (q.length) {
    const [x, y] = q.pop() as [number, number]
    if (x < 0 || y < 0 || x >= ctx.width || y >= ctx.height) continue
    const key = idx(x, y)
    if (visited[key]) continue
    visited[key] = 1
    if (!same(ctx.getPixel(x, y), source)) continue
    ctx.setPixel(x, y, target)
    changed = true
    q.push([x + 1, y], [x - 1, y], [x, y + 1], [x, y - 1])
  }

  return { changed }
}
