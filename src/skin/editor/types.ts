export type ToolId = 'brush' | 'eraser' | 'eyedropper' | 'fill' | 'rect'

export type RGBA = { r: number; g: number; b: number; a: number }

export type PaintPoint = { x: number; y: number }

export type BrushSettings = {
  size: number
  hardness: number
  opacity: number
  symmetry: boolean
}

export type SelectionRect = { x: number; y: number; width: number; height: number } | null

export type PaintContext = {
  width: number
  height: number
  getPixel: (x: number, y: number) => RGBA
  setPixel: (x: number, y: number, color: RGBA) => void
  selection: SelectionRect
}

export type ToolActionResult = {
  changed: boolean
  pickedColor?: string
}
