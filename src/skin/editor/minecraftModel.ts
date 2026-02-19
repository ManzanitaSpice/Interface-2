import * as THREE from 'three'

export type ModelVariant = 'classic' | 'slim'
export type SkinLayerKind = 'base' | 'overlay'
export type SkinPartKey = 'head' | 'body' | 'leftArm' | 'rightArm' | 'leftLeg' | 'rightLeg'

export type ModelLayersVisibility = Record<SkinPartKey, { base: boolean; overlay: boolean }>

type UV = { x: number; y: number; w: number; h: number }

type CuboidUv = {
  up: UV
  down: UV
  right: UV
  front: UV
  left: UV
  back: UV
}

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

const mapCuboid = (geo: THREE.BoxGeometry, uv: CuboidUv, tw: number, th: number) => {
  setFaceUv(geo, 0, uv.right, tw, th)
  setFaceUv(geo, 1, uv.left, tw, th)
  setFaceUv(geo, 2, uv.up, tw, th)
  setFaceUv(geo, 3, uv.down, tw, th)
  setFaceUv(geo, 4, uv.front, tw, th)
  setFaceUv(geo, 5, uv.back, tw, th)
}

const createPartMesh = (
  size: [number, number, number],
  pos: [number, number, number],
  uv: CuboidUv,
  textureWidth: number,
  textureHeight: number,
  material: THREE.Material,
): THREE.Mesh => {
  const geo = new THREE.BoxGeometry(size[0], size[1], size[2])
  mapCuboid(geo, uv, textureWidth, textureHeight)
  const mesh = new THREE.Mesh(geo, material)
  mesh.position.set(...pos)
  return mesh
}

export const defaultLayerVisibility = (): ModelLayersVisibility => ({
  head: { base: true, overlay: true },
  body: { base: true, overlay: true },
  leftArm: { base: true, overlay: true },
  rightArm: { base: true, overlay: true },
  leftLeg: { base: true, overlay: true },
  rightLeg: { base: true, overlay: true },
})

export const buildMinecraftModel = (
  texture: THREE.Texture,
  variant: ModelVariant,
  textureHeight: 64 | 128,
  visibility: ModelLayersVisibility,
) => {
  const root = new THREE.Group()
  const baseMaterial = new THREE.MeshStandardMaterial({ map: texture, transparent: true, alphaTest: 0.05, roughness: 0.95, metalness: 0.02 })
  const overlayMaterial = new THREE.MeshStandardMaterial({ map: texture, transparent: true, alphaTest: 0.05, roughness: 0.95, metalness: 0.02, side: THREE.DoubleSide })

  const armW = variant === 'slim' ? 3 : 4
  const armX = 4 + armW / 2
  const th = textureHeight

  const addLayeredPart = (
    part: SkinPartKey,
    baseSize: [number, number, number],
    overlaySize: [number, number, number],
    pos: [number, number, number],
    baseUv: CuboidUv,
    overlayUv: CuboidUv,
  ) => {
    const group = new THREE.Group()
    group.name = part

    const base = createPartMesh(baseSize, pos, baseUv, 64, th, baseMaterial)
    base.name = `${part}-base`
    base.visible = visibility[part].base
    group.add(base)

    const overlay = createPartMesh(overlaySize, pos, overlayUv, 64, th, overlayMaterial)
    overlay.name = `${part}-overlay`
    overlay.visible = visibility[part].overlay
    group.add(overlay)

    root.add(group)
  }

  addLayeredPart(
    'head',
    [8, 8, 8],
    [9, 9, 9],
    [0, 24, 0],
    { up: { x: 8, y: 0, w: 8, h: 8 }, down: { x: 16, y: 0, w: 8, h: 8 }, right: { x: 0, y: 8, w: 8, h: 8 }, front: { x: 8, y: 8, w: 8, h: 8 }, left: { x: 16, y: 8, w: 8, h: 8 }, back: { x: 24, y: 8, w: 8, h: 8 } },
    { up: { x: 40, y: 0, w: 8, h: 8 }, down: { x: 48, y: 0, w: 8, h: 8 }, right: { x: 32, y: 8, w: 8, h: 8 }, front: { x: 40, y: 8, w: 8, h: 8 }, left: { x: 48, y: 8, w: 8, h: 8 }, back: { x: 56, y: 8, w: 8, h: 8 } },
  )

  addLayeredPart(
    'body',
    [8, 12, 4],
    [8.5, 12.5, 4.5],
    [0, 14, 0],
    { up: { x: 20, y: 16, w: 8, h: 4 }, down: { x: 28, y: 16, w: 8, h: 4 }, right: { x: 16, y: 20, w: 4, h: 12 }, front: { x: 20, y: 20, w: 8, h: 12 }, left: { x: 28, y: 20, w: 4, h: 12 }, back: { x: 32, y: 20, w: 8, h: 12 } },
    { up: { x: 20, y: 32, w: 8, h: 4 }, down: { x: 28, y: 32, w: 8, h: 4 }, right: { x: 16, y: 36, w: 4, h: 12 }, front: { x: 20, y: 36, w: 8, h: 12 }, left: { x: 28, y: 36, w: 4, h: 12 }, back: { x: 32, y: 36, w: 8, h: 12 } },
  )

  addLayeredPart(
    'leftArm',
    [armW, 12, 4],
    [armW + 0.5, 12.5, 4.5],
    [-armX, 14, 0],
    { up: { x: 44, y: 16, w: armW, h: 4 }, down: { x: 44 + armW, y: 16, w: armW, h: 4 }, right: { x: 40, y: 20, w: 4, h: 12 }, front: { x: 44, y: 20, w: armW, h: 12 }, left: { x: 44 + armW, y: 20, w: 4, h: 12 }, back: { x: 48 + armW, y: 20, w: armW, h: 12 } },
    { up: { x: 44, y: 32, w: armW, h: 4 }, down: { x: 44 + armW, y: 32, w: armW, h: 4 }, right: { x: 40, y: 36, w: 4, h: 12 }, front: { x: 44, y: 36, w: armW, h: 12 }, left: { x: 44 + armW, y: 36, w: 4, h: 12 }, back: { x: 48 + armW, y: 36, w: armW, h: 12 } },
  )

  addLayeredPart(
    'rightArm',
    [armW, 12, 4],
    [armW + 0.5, 12.5, 4.5],
    [armX, 14, 0],
    { up: { x: 36, y: 48, w: armW, h: 4 }, down: { x: 36 + armW, y: 48, w: armW, h: 4 }, right: { x: 32, y: 52, w: 4, h: 12 }, front: { x: 36, y: 52, w: armW, h: 12 }, left: { x: 36 + armW, y: 52, w: 4, h: 12 }, back: { x: 40 + armW, y: 52, w: armW, h: 12 } },
    { up: { x: 52, y: 48, w: armW, h: 4 }, down: { x: 52 + armW, y: 48, w: armW, h: 4 }, right: { x: 48, y: 52, w: 4, h: 12 }, front: { x: 52, y: 52, w: armW, h: 12 }, left: { x: 52 + armW, y: 52, w: 4, h: 12 }, back: { x: 56 + armW, y: 52, w: armW, h: 12 } },
  )

  addLayeredPart(
    'leftLeg',
    [4, 12, 4],
    [4.5, 12.5, 4.5],
    [-2, 2, 0],
    { up: { x: 4, y: 16, w: 4, h: 4 }, down: { x: 8, y: 16, w: 4, h: 4 }, right: { x: 0, y: 20, w: 4, h: 12 }, front: { x: 4, y: 20, w: 4, h: 12 }, left: { x: 8, y: 20, w: 4, h: 12 }, back: { x: 12, y: 20, w: 4, h: 12 } },
    { up: { x: 4, y: 32, w: 4, h: 4 }, down: { x: 8, y: 32, w: 4, h: 4 }, right: { x: 0, y: 36, w: 4, h: 12 }, front: { x: 4, y: 36, w: 4, h: 12 }, left: { x: 8, y: 36, w: 4, h: 12 }, back: { x: 12, y: 36, w: 4, h: 12 } },
  )

  addLayeredPart(
    'rightLeg',
    [4, 12, 4],
    [4.5, 12.5, 4.5],
    [2, 2, 0],
    { up: { x: 20, y: 48, w: 4, h: 4 }, down: { x: 24, y: 48, w: 4, h: 4 }, right: { x: 16, y: 52, w: 4, h: 12 }, front: { x: 20, y: 52, w: 4, h: 12 }, left: { x: 24, y: 52, w: 4, h: 12 }, back: { x: 28, y: 52, w: 4, h: 12 } },
    { up: { x: 4, y: 48, w: 4, h: 4 }, down: { x: 8, y: 48, w: 4, h: 4 }, right: { x: 0, y: 52, w: 4, h: 12 }, front: { x: 4, y: 52, w: 4, h: 12 }, left: { x: 8, y: 52, w: 4, h: 12 }, back: { x: 12, y: 52, w: 4, h: 12 } },
  )

  root.position.y = -12
  return root
}
