import * as THREE from 'three'

export type ModelVariant = 'classic' | 'slim'

type UV = { x: number; y: number; w: number; h: number }

const setFaceUv = (geo: THREE.BoxGeometry, face: number, uv: UV, tw: number, th: number) => {
  const u0 = uv.x / tw
  const v0 = 1 - uv.y / th
  const u1 = (uv.x + uv.w) / tw
  const v1 = 1 - (uv.y + uv.h) / th
  const attr = geo.getAttribute('uv') as THREE.BufferAttribute
  const base = face * 4
  attr.setXY(base + 0, u1, v0)
  attr.setXY(base + 1, u0, v0)
  attr.setXY(base + 2, u1, v1)
  attr.setXY(base + 3, u0, v1)
  attr.needsUpdate = true
}

const mapCuboid = (geo: THREE.BoxGeometry, uv: { up: UV; down: UV; right: UV; front: UV; left: UV; back: UV }, tw: number, th: number) => {
  setFaceUv(geo, 0, uv.right, tw, th)
  setFaceUv(geo, 1, uv.left, tw, th)
  setFaceUv(geo, 2, uv.up, tw, th)
  setFaceUv(geo, 3, uv.down, tw, th)
  setFaceUv(geo, 4, uv.front, tw, th)
  setFaceUv(geo, 5, uv.back, tw, th)
}

export const buildMinecraftModel = (texture: THREE.Texture, variant: ModelVariant, textureHeight: 64 | 128) => {
  const root = new THREE.Group()
  const material = new THREE.MeshLambertMaterial({ map: texture, transparent: true, alphaTest: 0.01 })

  const armW = variant === 'slim' ? 3 : 4
  const th = textureHeight

  const addPart = (size: [number, number, number], pos: [number, number, number], uv: Parameters<typeof mapCuboid>[1]) => {
    const geo = new THREE.BoxGeometry(size[0], size[1], size[2])
    mapCuboid(geo, uv, 64, th)
    const mesh = new THREE.Mesh(geo, material)
    mesh.position.set(...pos)
    root.add(mesh)
  }

  addPart([8, 8, 8], [0, 24, 0], { up: { x: 8, y: 0, w: 8, h: 8 }, down: { x: 16, y: 0, w: 8, h: 8 }, right: { x: 0, y: 8, w: 8, h: 8 }, front: { x: 8, y: 8, w: 8, h: 8 }, left: { x: 16, y: 8, w: 8, h: 8 }, back: { x: 24, y: 8, w: 8, h: 8 } })
  addPart([8, 12, 4], [0, 14, 0], { up: { x: 20, y: 16, w: 8, h: 4 }, down: { x: 28, y: 16, w: 8, h: 4 }, right: { x: 16, y: 20, w: 4, h: 12 }, front: { x: 20, y: 20, w: 8, h: 12 }, left: { x: 28, y: 20, w: 4, h: 12 }, back: { x: 32, y: 20, w: 8, h: 12 } })
  addPart([armW, 12, 4], [-(4 + armW / 2), 14, 0], { up: { x: 44, y: 16, w: armW, h: 4 }, down: { x: 44 + armW, y: 16, w: armW, h: 4 }, right: { x: 40, y: 20, w: 4, h: 12 }, front: { x: 44, y: 20, w: armW, h: 12 }, left: { x: 44 + armW, y: 20, w: 4, h: 12 }, back: { x: 48 + armW, y: 20, w: armW, h: 12 } })
  addPart([armW, 12, 4], [4 + armW / 2, 14, 0], { up: { x: 36, y: 48, w: armW, h: 4 }, down: { x: 36 + armW, y: 48, w: armW, h: 4 }, right: { x: 32, y: 52, w: 4, h: 12 }, front: { x: 36, y: 52, w: armW, h: 12 }, left: { x: 36 + armW, y: 52, w: 4, h: 12 }, back: { x: 40 + armW, y: 52, w: armW, h: 12 } })
  addPart([4, 12, 4], [-2, 2, 0], { up: { x: 4, y: 16, w: 4, h: 4 }, down: { x: 8, y: 16, w: 4, h: 4 }, right: { x: 0, y: 20, w: 4, h: 12 }, front: { x: 4, y: 20, w: 4, h: 12 }, left: { x: 8, y: 20, w: 4, h: 12 }, back: { x: 12, y: 20, w: 4, h: 12 } })
  addPart([4, 12, 4], [2, 2, 0], { up: { x: 20, y: 48, w: 4, h: 4 }, down: { x: 24, y: 48, w: 4, h: 4 }, right: { x: 16, y: 52, w: 4, h: 12 }, front: { x: 20, y: 52, w: 4, h: 12 }, left: { x: 24, y: 52, w: 4, h: 12 }, back: { x: 28, y: 52, w: 4, h: 12 } })

  root.position.y = -12
  return root
}
