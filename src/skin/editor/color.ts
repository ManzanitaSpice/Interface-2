import type { RGBA } from './types'

export const hexToRgba = (hex: string, alpha = 1): RGBA => {
  const clean = hex.replace('#', '')
  const value = clean.length === 3 ? clean.split('').map((v) => v + v).join('') : clean
  const n = Number.parseInt(value, 16)
  return {
    r: (n >> 16) & 255,
    g: (n >> 8) & 255,
    b: n & 255,
    a: Math.round(Math.max(0, Math.min(1, alpha)) * 255),
  }
}

export const rgbaToHex = ({ r, g, b }: RGBA) => `#${[r, g, b].map((v) => v.toString(16).padStart(2, '0')).join('')}`

export const blend = (base: RGBA, top: RGBA): RGBA => {
  const aTop = top.a / 255
  const aBase = base.a / 255
  const outA = aTop + aBase * (1 - aTop)
  if (outA <= 0) return { r: 0, g: 0, b: 0, a: 0 }
  return {
    r: Math.round((top.r * aTop + base.r * aBase * (1 - aTop)) / outA),
    g: Math.round((top.g * aTop + base.g * aBase * (1 - aTop)) / outA),
    b: Math.round((top.b * aTop + base.b * aBase * (1 - aTop)) / outA),
    a: Math.round(outA * 255),
  }
}

export const rgbToHsv = (r: number, g: number, b: number) => {
  const rn = r / 255
  const gn = g / 255
  const bn = b / 255
  const max = Math.max(rn, gn, bn)
  const min = Math.min(rn, gn, bn)
  const d = max - min
  let h = 0
  if (d !== 0) {
    if (max === rn) h = ((gn - bn) / d) % 6
    else if (max === gn) h = (bn - rn) / d + 2
    else h = (rn - gn) / d + 4
  }
  return { h: Math.round(h * 60 < 0 ? h * 60 + 360 : h * 60), s: max === 0 ? 0 : d / max, v: max }
}

export const hsvToRgb = (h: number, s: number, v: number) => {
  const c = v * s
  const x = c * (1 - Math.abs((h / 60) % 2 - 1))
  const m = v - c
  let r = 0, g = 0, b = 0
  if (h < 60) [r, g, b] = [c, x, 0]
  else if (h < 120) [r, g, b] = [x, c, 0]
  else if (h < 180) [r, g, b] = [0, c, x]
  else if (h < 240) [r, g, b] = [0, x, c]
  else if (h < 300) [r, g, b] = [x, 0, c]
  else [r, g, b] = [c, 0, x]
  return { r: Math.round((r + m) * 255), g: Math.round((g + m) * 255), b: Math.round((b + m) * 255) }
}
