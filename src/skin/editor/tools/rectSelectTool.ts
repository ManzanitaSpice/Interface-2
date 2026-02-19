import type { PaintPoint, SelectionRect } from '../types'

export const buildSelection = (start: PaintPoint, end: PaintPoint): SelectionRect => {
  const x = Math.min(start.x, end.x)
  const y = Math.min(start.y, end.y)
  const width = Math.abs(end.x - start.x) + 1
  const height = Math.abs(end.y - start.y) + 1
  if (width <= 0 || height <= 0) return null
  return { x, y, width, height }
}
