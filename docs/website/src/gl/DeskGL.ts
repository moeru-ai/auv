// WebGL renderer for a SceneState.
//
// Frame:
//   1. background (theme color + faint base field), the opening's display
//      cards behind the desk, and per-window glows
//   2. windows back to front; before each glass window, blur what has been
//      drawn so far (1/8-res pyramid + gaussian) and let the glass sample it
//      in screen space. This is backdrop-filter, done once per window on the GPU.
//   3. cursors, labels, ripples, then display cards in front of the desk
//   4. composite to the canvas, optionally through a frosted veil with a
//      revealed hole (landing background)
//
// Windows and display cards are parallel planes. Windows sit a hair apart in a
// fixed order and cards are sorted by view depth, so painter's order is exact
// in any orthographic view and no depth buffer is needed.

import type { Blob, Camera, CursorNode, DisplayNode, Ripple, SceneState, Vec3, WindowNode } from '../scene/types'
import type { Theme } from '../theme'

import { clamp } from 'es-toolkit'

import * as THREE from 'three'

import { parse } from '../lib/color'
import { GHOST_HOTSPOT, USER_HOTSPOT } from '../lib/icon'
import { slabCommands } from '../lib/slab'
import { GHOST_SCALE, USER_SCALE } from '../render/Overlay'
import { ContentTextures, CURSOR_BOX, cursorTexture, LABEL_H, labelTexture } from './textures'

THREE.ColorManagement.enabled = false

const RADIUS = 18
/** Gaussian sigma in stage px for window glass and for the landing's frosted veil. */
const GLASS_SIGMA = 24
const VEIL_SIGMA = 20

export interface Fit {
  /** CSS px per stage px. */
  k: number
  /** CSS offset of the stage's top-left inside the canvas. */
  x: number
  y: number
}

export interface RenderOptions {
  fit: Fit
  /** Only this window gets real glass; others use a flat tint. Undefined = all glass. */
  focus?: null | string
  /** Stage size in stage px (defaults to the 1920x1080 landscape film). */
  stage?: { h: number, w: number }
  veil?: Veil
}

export interface Veil {
  amount: number
  hole: { h: number, w: number, x: number, y: number }
  open: number
  tint: string
}

function vec4(c: string) {
  const [r, g, b, a] = parse(c)
  return new THREE.Vector4(r / 255, g / 255, b / 255, a)
}

const FULL_VERT = /* glsl */`
out vec2 vUv;
void main() { vUv = uv; gl_Position = vec4(position.xy, 0.0, 1.0); }`

const DOWN_FRAG = /* glsl */`
uniform sampler2D tSrc; uniform vec2 uTexel; in vec2 vUv; out vec4 o;
void main() {
  o = 0.25 * (texture(tSrc, vUv + uTexel * vec2(-0.5, -0.5)) + texture(tSrc, vUv + uTexel * vec2(0.5, -0.5))
            + texture(tSrc, vUv + uTexel * vec2(-0.5, 0.5)) + texture(tSrc, vUv + uTexel * vec2(0.5, 0.5)));
}`

// 9-tap gaussian folded into 5 bilinear fetches.
const GAUSS_FRAG = /* glsl */`
uniform sampler2D tSrc; uniform vec2 uDir; in vec2 vUv; out vec4 o;
void main() {
  o = texture(tSrc, vUv) * 0.2270270270;
  o += (texture(tSrc, vUv + uDir * 1.3846153846) + texture(tSrc, vUv - uDir * 1.3846153846)) * 0.3162162162;
  o += (texture(tSrc, vUv + uDir * 3.2307692308) + texture(tSrc, vUv - uDir * 3.2307692308)) * 0.0702702703;
}`

// Maps gl_FragCoord to stage px for screen-space effects.
const STAGE_FN = /* glsl */`
uniform vec4 uFrag; // (1/(dpr*k), bufferHeight, offsetX/k, offsetY/k)
vec2 stagePos() { return vec2(gl_FragCoord.x, uFrag.y - gl_FragCoord.y) * uFrag.x - uFrag.zw; }`

const BG_FRAG = /* glsl */`
${STAGE_FN}
uniform vec3 uBg; uniform vec4 uBlob[4]; uniform vec4 uBlobColor[4]; out vec4 o;
void main() {
  vec2 p = stagePos();
  vec3 c = uBg;
  for (int i = 0; i < 4; i++) {
    float r = length(p - uBlob[i].xy) / max(uBlob[i].z, 1.0);
    float a = r < 0.6 ? mix(1.0, 0.35, r / 0.6) : mix(0.35, 0.0, clamp((r - 0.6) / 0.4, 0.0, 1.0));
    c = mix(c, uBlobColor[i].rgb, a * uBlobColor[i].a);
  }
  o = vec4(c, 1.0);
}`

const WORLD_VERT = /* glsl */`
out vec2 vUv;
void main() { vUv = uv; gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0); }`

const SDF = /* glsl */`
float sdRound(vec2 p, vec2 b, float r) { vec2 q = abs(p) - b + r; return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r; }`

const GLASS_FRAG = /* glsl */`
${SDF}
uniform sampler2D tBlur; uniform vec2 uViewport; uniform vec2 uSize; uniform float uRadius;
uniform vec4 uTint; uniform vec4 uEdge; uniform vec4 uHigh; uniform float uSat; uniform float uBright;
uniform float uUseBlur; uniform float uUseSdf; uniform float uOpacity;
in vec2 vUv; out vec4 o;
void main() {
  vec2 p = (vUv - 0.5) * uSize;
  float d = uUseSdf > 0.5 ? sdRound(p, uSize * 0.5, uRadius) : -10.0;
  float aa = max(fwidth(d), 1e-3);
  float a = 1.0 - smoothstep(-aa, aa, d);
  vec3 col;
  float alpha;
  if (uUseBlur > 0.5) {
    vec3 back = texture(tBlur, gl_FragCoord.xy / uViewport).rgb;
    float l = dot(back, vec3(0.299, 0.587, 0.114));
    back = mix(vec3(l), back, uSat) * uBright;
    col = mix(back, uTint.rgb, uTint.a);
    alpha = 1.0;
  } else {
    col = uTint.rgb;
    alpha = uTint.a;
  }
  if (uUseSdf > 0.5) {
    float ring = 1.0 - smoothstep(0.0, 1.2 * aa, abs(d + 0.6));
    col = mix(col, uEdge.rgb, ring * uEdge.a);
    alpha = max(alpha, ring * uEdge.a);
    float top = smoothstep(uSize.y * 0.5 - 2.2, uSize.y * 0.5 - 1.0, p.y) * (1.0 - smoothstep(-2.5, -1.0, d));
    col = mix(col, uHigh.rgb, top * uHigh.a);
  }
  o = vec4(col, alpha * a * uOpacity);
}`

const SHADOW_FRAG = /* glsl */`
${SDF}
uniform vec2 uSize; uniform vec2 uInner; uniform float uRadius; uniform float uSoft; uniform vec4 uColor; uniform float uOpacity;
in vec2 vUv; out vec4 o;
void main() {
  vec2 p = (vUv - 0.5) * uSize;
  float d = sdRound(p, uInner * 0.5, uRadius);
  float a = 1.0 - smoothstep(-uSoft * 0.5, uSoft, d);
  o = vec4(uColor.rgb, uColor.a * a * a * uOpacity);
}`

const GLOW_FRAG = /* glsl */`
uniform vec4 uColor; in vec2 vUv; out vec4 o;
void main() {
  float r = length(vUv - 0.5) * 2.0;
  float a = r < 0.6 ? mix(1.0, 0.35, r / 0.6) : mix(0.35, 0.0, clamp((r - 0.6) / 0.4, 0.0, 1.0));
  o = vec4(uColor.rgb, uColor.a * a);
}`

const RIPPLE_FRAG = /* glsl */`
uniform vec4 uColor; uniform float uP; uniform float uExtent; in vec2 vUv; out vec4 o;
void main() {
  float d = length(vUv - 0.5) * 2.0 * uExtent;
  float r = 8.0 + 46.0 * uP;
  float w = 4.0 * (1.0 - uP) + 1.0;
  float aa = fwidth(d);
  float ring = 1.0 - smoothstep(w * 0.5 - aa, w * 0.5 + aa, abs(d - r));
  float disc = 1.0 - smoothstep(14.0 * (1.0 - uP) - aa, 14.0 * (1.0 - uP) + aa, d);
  float a = max(ring, disc * 0.35) * (1.0 - uP) * uColor.a;
  o = vec4(uColor.rgb, a);
}`

// Faint display card: brightest at its center, fading toward the rim, plus a
// hairline rim. Defocus feathers the edge by uSoft (world px) and melts the rim:
// for a flat card that is close to a real blur, without a blur pass per card.
const DISPLAY_FRAG = /* glsl */`
${SDF}
uniform vec2 uSize; uniform float uRadius; uniform vec4 uFill; uniform vec4 uRim; uniform float uOpacity; uniform float uSoft;
in vec2 vUv; out vec4 o;
void main() {
  vec2 p = (vUv - 0.5) * uSize;
  float d = sdRound(p, uSize * 0.5 - uSoft, uRadius);
  float aa = max(fwidth(d), 1e-3);
  float edge = aa + uSoft;
  float inside = 1.0 - smoothstep(-edge, edge, d);
  float r = length(vUv - 0.5) * 2.0;
  float fill = uFill.a * mix(1.0, 0.3, smoothstep(0.0, 1.25, r));
  float ring = (1.0 - smoothstep(0.0, 1.5 * aa + uSoft, abs(d + 1.0))) * uRim.a * aa / (aa + 0.25 * uSoft);
  float a = max(fill, ring);
  o = vec4(mix(uFill.rgb, uRim.rgb, ring / max(a, 1e-4)), a * inside * uOpacity);
}`

const COMPOSITE_FRAG = /* glsl */`
${STAGE_FN}
${SDF}
uniform sampler2D tSharp; uniform sampler2D tBlur; uniform vec2 uViewport;
uniform float uVeil; uniform vec4 uTint; uniform vec4 uHole; uniform float uOpen; out vec4 o;
void main() {
  vec2 uv = gl_FragCoord.xy / uViewport;
  vec4 sharp = texture(tSharp, uv);
  if (uVeil <= 0.0) { o = sharp; return; }
  vec2 p = stagePos();
  vec2 c = uHole.xy + uHole.zw * 0.5;
  vec2 half_ = uHole.zw * 0.5 * uOpen;
  float d = sdRound(p - c, max(half_, vec2(0.001)), min(26.0, min(half_.x, half_.y)));
  float hole = (1.0 - smoothstep(-6.0, 10.0, d)) * step(0.01, uOpen);
  vec3 frosted = mix(texture(tBlur, uv).rgb, uTint.rgb, uTint.a * uVeil);
  o = vec4(mix(sharp.rgb, frosted, uVeil * (1.0 - hole)), 1.0);
}`

interface WindowMeshes {
  content: THREE.Mesh<THREE.PlaneGeometry, THREE.MeshBasicMaterial>
  glass: THREE.Mesh<THREE.BufferGeometry, THREE.ShaderMaterial>
  group: THREE.Group
  shadow: THREE.Mesh<THREE.PlaneGeometry, THREE.ShaderMaterial>
  slabKey: string
}

export class DeskGL {
  readonly renderer: THREE.WebGLRenderer
  private bg = fullscreen(BG_FRAG, {
    uBg: { value: new THREE.Vector3() },
    uBlob: { value: Array.from({ length: 4 }, () => new THREE.Vector4()) },
    uBlobColor: { value: Array.from({ length: 4 }, () => new THREE.Vector4()) },
    uFrag: { value: new THREE.Vector4() },
  })

  private camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 1, 20000)
  private composite = fullscreen(COMPOSITE_FRAG, {
    tBlur: { value: null },
    tSharp: { value: null },
    uFrag: { value: new THREE.Vector4() },
    uHole: { value: new THREE.Vector4() },
    uOpen: { value: 0 },
    uTint: { value: new THREE.Vector4() },
    uVeil: { value: 0 },
    uViewport: { value: new THREE.Vector2() },
  })

  private content = new ContentTextures()
  private displayGroup = new THREE.Group()
  private displayPool: THREE.Mesh<THREE.PlaneGeometry, THREE.ShaderMaterial>[] = []
  private down = fullscreen(DOWN_FRAG, { tSrc: { value: null }, uTexel: { value: new THREE.Vector2() } })
  /** Eye distance of the current frame's camera. */
  private eyeDist = 6000
  private fitK = 1
  private fsCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1)
  private gauss = fullscreen(GAUSS_FRAG, { tSrc: { value: null }, uDir: { value: new THREE.Vector2() } })
  private glowGroup = new THREE.Group()
  private glowPool: THREE.Mesh<THREE.PlaneGeometry, THREE.ShaderMaterial>[] = []
  private height = 0

  private persp = new THREE.PerspectiveCamera()

  private pingpong: THREE.WebGLRenderTarget[] = []
  private pixelRatio = 1
  private pyramid: THREE.WebGLRenderTarget[] = []
  private quad = new THREE.PlaneGeometry(1, 1)
  private ripplePool: THREE.Mesh<THREE.PlaneGeometry, THREE.ShaderMaterial>[] = []
  /** 4x MSAA twin of `rtSharp`, created on first use. */
  private rtMsaa: null | THREE.WebGLRenderTarget = null
  /** Scene target of the current frame: `rtSharp`, or `rtMsaa` when geometry edges need it. */
  private rtScene!: THREE.WebGLRenderTarget
  private rtSharp!: THREE.WebGLRenderTarget
  private scene = new THREE.Scene()
  private sh = 1080
  private spritePool: THREE.Mesh<THREE.PlaneGeometry, THREE.MeshBasicMaterial>[] = []
  private sw = 1920
  private topGroup = new THREE.Group()
  /** Camera of the current frame: flat `camera`, or `persp` for the opening. */
  private view: THREE.Camera = this.camera
  private width = 0
  private windows = new Map<string, WindowMeshes>()

  constructor(canvas: HTMLCanvasElement) {
    this.renderer = new THREE.WebGLRenderer({ alpha: false, antialias: false, canvas, powerPreference: 'high-performance', preserveDrawingBuffer: false })
    this.renderer.outputColorSpace = THREE.LinearSRGBColorSpace
    this.renderer.autoClear = false
    this.scene.add(this.displayGroup, this.glowGroup, this.topGroup)
  }

  dispose() {
    this.content.dispose()
    for (const rt of [this.rtSharp, this.rtMsaa, ...this.pyramid, ...this.pingpong]) rt?.dispose()
    this.renderer.dispose()
  }

  render(state: SceneState, theme: Theme, opts: RenderOptions) {
    const r = this.renderer
    const { fit } = opts
    this.fitK = fit.k
    this.sw = opts.stage?.w ?? 1920
    this.sh = opts.stage?.h ?? 1080
    this.rtScene = this.sceneTarget(state)
    const bufH = this.rtScene.height
    const fragU = new THREE.Vector4(1 / (this.pixelRatio * fit.k), bufH, fit.x / fit.k, fit.y / fit.k)
    this.setCamera(state.camera, fit)

    // 1. Background.
    const bgc = vec4(theme.bg)
    const bu = this.bg.mat.uniforms
    bu.uFrag.value.copy(fragU)
    bu.uBg.value.set(bgc.x, bgc.y, bgc.z)
    for (let i = 0; i < 4; i++) {
      const b = state.blobs[i]
      if (b) {
        bu.uBlob.value[i].set(b.x, b.y, b.r, 0)
        const c = vec4(typeof b.color === 'number' ? theme.blobs[b.color % theme.blobs.length] : b.color)
        c.w = (typeof b.color === 'number' ? theme.blobOpacity : theme.glow) * b.alpha
        bu.uBlobColor.value[i].copy(c)
      }
      else {
        bu.uBlobColor.value[i].set(0, 0, 0, 0)
      }
    }
    r.setRenderTarget(this.rtScene)
    r.setClearColor(new THREE.Color(bgc.x, bgc.y, bgc.z), 1)
    r.clear()
    r.render(this.bg.scene, this.fsCamera)

    const depth = (w: { depth?: number, z: number }) => w.z * 0.01 + (w.depth ?? 0)
    const windows = state.windows.slice().sort((a, b) => a.z - b.z)
    const byId = new Map(windows.map(w => [w.id, w]))
    // Glows are light cast on the desk itself: the back-most plane of the stack,
    // behind every window, at each window's position.
    const glows = state.glows ?? []
    // Right under the windows: in a turned view a deeper plane would slide the glows off them.
    const deskPlane = -1
    const winDepth = () => deskPlane
    const cards = this.placeDisplays(state.displays ?? [], state.camera, theme)
    this.drawDisplays(cards.far)
    this.placeGlows(glows, theme, winDepth)
    this.only(this.glowGroup)
    r.setRenderTarget(this.rtScene)
    r.render(this.scene, this.view)

    // 2. Windows, back to front.
    const contentScale = clamp(this.pixelRatio * fit.k, 1, 2.5)
    for (const [id, m] of this.windows) {
      if (!byId.has(id)) {
        m.group.visible = false
      }
    }
    for (const win of windows) {
      if (win.opacity <= 0.001)
        continue
      const m = this.windowMeshes(win.id)
      const glassOn = opts.focus === undefined || opts.focus === win.id
      const blur = glassOn ? this.blurScene(GLASS_SIGMA) : null
      this.placeWindow(win, m, depth(win), theme, glassOn, blur, contentScale)
      this.only(m.group)
      r.setRenderTarget(this.rtScene)
      r.render(this.scene, this.view)
    }
    this.content.retain(new Set(windows.map(w => w.id)))

    // 3. Cursors and ripples, in front of every window and just above the desk
    // plane, so they stay on it in a turned view.
    this.placeTop(state.cursors, state.ripples, 0.5)
    this.only(this.topGroup)
    r.setRenderTarget(this.rtScene)
    r.render(this.scene, this.view)
    this.drawDisplays(cards.near)

    // 4. Composite, with the optional frosted veil.
    const cu = this.composite.mat.uniforms
    cu.uFrag.value.copy(fragU)
    cu.tSharp.value = this.rtScene.texture
    cu.uViewport.value.set(this.rtScene.width, this.rtScene.height)
    const veil = opts.veil
    if (veil && veil.amount > 0.001) {
      cu.tBlur.value = this.blurScene(VEIL_SIGMA)
      cu.uVeil.value = veil.amount
      cu.uTint.value.copy(vec4(veil.tint))
      cu.uHole.value.set(veil.hole.x, veil.hole.y, veil.hole.w, veil.hole.h)
      cu.uOpen.value = veil.open
    }
    else {
      cu.uVeil.value = 0
    }
    r.setRenderTarget(null)
    r.render(this.composite.scene, this.fsCamera)
  }

  /** Size in CSS px. Pixel ratio is capped so the buffer stays near 4K. */
  setSize(cssW: number, cssH: number, dpr: number, maxPixels = 3840 * 2160) {
    const pr = Math.min(dpr, Math.sqrt(maxPixels / Math.max(1, cssW * cssH)))
    if (cssW === this.width && cssH === this.height && pr === this.pixelRatio)
      return
    this.width = cssW
    this.height = cssH
    this.pixelRatio = pr
    this.renderer.setPixelRatio(pr)
    this.renderer.setSize(cssW, cssH, false)
    const bw = Math.round(cssW * pr)
    const bh = Math.round(cssH * pr)
    for (const rt of [this.rtSharp, this.rtMsaa, ...this.pyramid, ...this.pingpong]) rt?.dispose()
    const opts = { depthBuffer: false, magFilter: THREE.LinearFilter, minFilter: THREE.LinearFilter, type: THREE.HalfFloatType } as const
    this.rtSharp = new THREE.WebGLRenderTarget(bw, bh, opts)
    this.rtMsaa = null
    this.rtScene = this.rtSharp
    this.pyramid = [2, 4, 8].map(d => new THREE.WebGLRenderTarget(Math.max(1, Math.round(bw / d)), Math.max(1, Math.round(bh / d)), opts))
    const last = this.pyramid[2]
    this.pingpong = [0, 1].map(() => new THREE.WebGLRenderTarget(last.width, last.height, opts))
  }

  /**
   * Blur the current scene target into pingpong[0]. `sigma` is in stage px, so
   * the look is the same at any device pixel ratio or fit scale.
   */
  private blurScene(sigma: number) {
    // One 1/8-res texel in stage px; each H+V pass adds ~1.4 * spread texels of sigma.
    const texel = 8 / (this.pixelRatio * this.fitK)
    const want = sigma / texel
    let passes = 2
    while (want / (1.4 * Math.sqrt(passes)) > 1.8 && passes < 8)
      passes++
    const spread = Math.max(0.6, want / (1.4 * Math.sqrt(passes)))
    const r = this.renderer
    let src: THREE.Texture = this.rtScene.texture
    let sw = this.rtScene.width
    let sh = this.rtScene.height
    for (const rt of this.pyramid) {
      this.down.mat.uniforms.tSrc.value = src
      this.down.mat.uniforms.uTexel.value.set(1 / sw, 1 / sh)
      r.setRenderTarget(rt)
      r.render(this.down.scene, this.fsCamera)
      src = rt.texture
      sw = rt.width
      sh = rt.height
    }
    const [a, b] = this.pingpong
    for (let i = 0; i < passes; i++) {
      this.gauss.mat.uniforms.tSrc.value = src
      this.gauss.mat.uniforms.uDir.value.set(spread / sw, 0)
      r.setRenderTarget(b)
      r.render(this.gauss.scene, this.fsCamera)
      this.gauss.mat.uniforms.tSrc.value = b.texture
      this.gauss.mat.uniforms.uDir.value.set(0, spread / sh)
      r.setRenderTarget(a)
      r.render(this.gauss.scene, this.fsCamera)
      src = a.texture
    }
    return a.texture
  }

  /** Draw only these display cards. */
  private drawDisplays(meshes: THREE.Mesh[]) {
    if (!meshes.length)
      return
    for (const m of this.displayPool) m.visible = meshes.includes(m)
    this.only(this.displayGroup)
    this.renderer.setRenderTarget(this.rtScene)
    this.renderer.render(this.scene, this.view)
  }

  private only(...visible: THREE.Object3D[]) {
    for (const child of this.scene.children) child.visible = visible.includes(child)
  }

  /**
   * Place display cards and split them around the desk: cards farther from the
   * eye than the desk display draw before the desk, nearer ones after it.
   */
  private placeDisplays(displays: DisplayNode[], cam: Camera | undefined, theme: Theme) {
    while (this.displayPool.length < displays.length) {
      const m = new THREE.Mesh(this.quad, worldMat(DISPLAY_FRAG, {
        uFill: { value: new THREE.Vector4() },
        uOpacity: { value: 1 },
        uRadius: { value: 0 },
        uRim: { value: new THREE.Vector4() },
        uSize: { value: new THREE.Vector2() },
        uSoft: { value: 0 },
      }))
      m.frustumCulled = false
      this.displayPool.push(m)
      this.displayGroup.add(m)
    }
    const dist = this.eyeDist
    const eye = cameraEye(cam, dist)
    const at: Vec3 = cam?.at ?? [0, 0, 0]
    const fwd = new THREE.Vector3(at[0] - eye[0], at[1] - eye[1], at[2] - eye[2]).normalize()
    const depthOf = (c: Vec3) => (c[0] - eye[0]) * fwd.x + (c[1] - eye[1]) * fwd.y + (c[2] - eye[2]) * fwd.z
    const deskDepth = depthOf([0, 0, 0])
    const far: THREE.Mesh[] = []
    const near: THREE.Mesh[] = []
    const order = displays.map((d, i) => ({ d, depth: depthOf(d.center), i })).sort((a, b) => b.depth - a.depth)
    order.forEach(({ d, depth, i }, rank) => {
      const m = this.displayPool[i]
      m.position.set(...d.center)
      m.scale.set(d.w, d.h, 1)
      m.renderOrder = rank
      const u = m.material.uniforms
      u.uSize.value.set(d.w, d.h)
      u.uRadius.value = d.radius
      u.uFill.value.copy(vec4(theme.display))
      u.uRim.value.copy(vec4(theme.displayRim))
      // Cards closing in on the eye fade before they can fill the frame.
      u.uOpacity.value = d.opacity * THREE.MathUtils.smoothstep(depth, 0.25 * dist, 0.7 * dist)
      u.uSoft.value = Math.min(d.soft, 0.45 * Math.min(d.w, d.h))
      if (u.uOpacity.value > 0.002)
        (depth >= deskDepth ? far : near).push(m)
    })
    return { far, near }
  }

  private placeGlows(glows: Blob[], theme: Theme, depthOf: (b: Blob) => number) {
    while (this.glowPool.length < glows.length) {
      const g = new THREE.Mesh(this.quad, worldMat(GLOW_FRAG, { uColor: { value: new THREE.Vector4() } }))
      g.frustumCulled = false
      this.glowPool.push(g)
      this.glowGroup.add(g)
    }
    this.glowPool.forEach((g, i) => {
      const b = glows[i]
      g.visible = !!b
      if (!b)
        return
      g.position.set(b.x - this.sw / 2, this.sh / 2 - b.y, depthOf(b))
      g.scale.set(b.r * 2, b.r * 2, 1)
      const c = vec4(String(b.color))
      c.w = theme.glow * b.alpha
      g.material.uniforms.uColor.value.copy(c)
    })
  }

  private placeTop(cursors: CursorNode[], ripples: Ripple[], z: number) {
    const sprites: { h: number, opacity: number, tex: THREE.Texture, w: number, x: number, y: number }[] = []
    for (const c of cursors) {
      if (c.outline || c.opacity <= 0.001)
        continue
      if (c.kind === 'ghost' && c.label && c.label.pop > 0.001) {
        const text = c.label.text.slice(0, c.label.chars)
        const l = labelTexture(c.color, text)
        // Flip to the cursor's left when the pill would run off the stage.
        const flip = c.x + 26 + l.w > this.sw - 16
        const lw = l.w * c.label.pop
        sprites.push({ h: LABEL_H * c.label.pop, opacity: c.opacity * Math.min(1, c.label.pop * 1.5), tex: l.tex, w: lw, x: flip ? c.x - 14 - lw : c.x + 26, y: c.y + 40 })
      }
      const ghost = c.kind === 'ghost'
      const s = (ghost ? GHOST_SCALE : USER_SCALE) * c.scale
      const hot = ghost ? GHOST_HOTSPOT : USER_HOTSPOT
      sprites.push({
        h: CURSOR_BOX.h * s,
        opacity: c.opacity,
        tex: cursorTexture(c.kind, c.color),
        w: CURSOR_BOX.w * s,
        x: c.x + (CURSOR_BOX.x - hot.x) * s,
        y: c.y + (CURSOR_BOX.y - hot.y) * s,
      })
    }
    while (this.spritePool.length < sprites.length) {
      const m = new THREE.Mesh(this.quad, new THREE.MeshBasicMaterial({ depthTest: false, depthWrite: false, transparent: true }))
      m.frustumCulled = false
      this.spritePool.push(m)
      this.topGroup.add(m)
    }
    this.spritePool.forEach((m, i) => {
      const s = sprites[i]
      m.visible = !!s
      if (!s)
        return
      if (m.material.map !== s.tex) {
        m.material.map = s.tex
        m.material.needsUpdate = true
      }
      m.material.opacity = s.opacity
      m.position.set(s.x + s.w / 2 - this.sw / 2, this.sh / 2 - (s.y + s.h / 2), z + 2 + i * 0.01)
      m.scale.set(s.w, s.h, 1)
      m.renderOrder = 10 + i
    })

    while (this.ripplePool.length < ripples.length) {
      const m = new THREE.Mesh(this.quad, worldMat(RIPPLE_FRAG, { uColor: { value: new THREE.Vector4() }, uExtent: { value: 60 }, uP: { value: 0 } }))
      m.frustumCulled = false
      this.ripplePool.push(m)
      this.topGroup.add(m)
    }
    this.ripplePool.forEach((m, i) => {
      const r = ripples[i]
      m.visible = !!r
      if (!r)
        return
      m.position.set(r.x - this.sw / 2, this.sh / 2 - r.y, z + 1)
      m.scale.set(120, 120, 1)
      m.material.uniforms.uColor.value.copy(vec4(r.color))
      m.material.uniforms.uP.value = r.p
      m.renderOrder = 5
    })
  }

  private placeWindow(win: WindowNode, m: WindowMeshes, z: number, theme: Theme, glassOn: boolean, blur: null | THREE.Texture, contentScale: number) {
    const s = win.scale ?? 1
    const o = win.origin ?? { x: win.rect.w / 2, y: win.rect.h / 2 }
    // Uniform scale about the window-local origin, done on the rect.
    const ax = win.rect.x + o.x
    const ay = win.rect.y + o.y
    const x = ax + (win.rect.x - ax) * s
    const y = ay + (win.rect.y - ay) * s
    const w = win.rect.w * s
    const h = win.rect.h * s
    const cx = x + w / 2 - this.sw / 2
    const cy = this.sh / 2 - (y + h / 2)
    const dark = win.kind === 'term'
    const u = m.glass.material.uniforms

    if (win.slab || win.poly) {
      const key = win.slab ? JSON.stringify(win.slab) : win.poly!.map(p => `${p[0].toFixed(1)},${p[1].toFixed(1)}`).join(' ')
      if (key !== m.slabKey) {
        const shape = new THREE.Shape()
        if (win.poly) {
          win.poly.forEach(([px, py], i) => (i === 0 ? shape.moveTo(px - this.sw / 2, this.sh / 2 - py) : shape.lineTo(px - this.sw / 2, this.sh / 2 - py)))
        }
        for (const c of win.slab ? slabCommands(win.slab) : []) {
          const P = (p: [number, number]) => [p[0] - this.sw / 2, this.sh / 2 - p[1]] as const
          if (c.op === 'C')
            shape.bezierCurveTo(...P(c.c1), ...P(c.c2), ...P(c.p))
          else if (c.op === 'M')
            shape.moveTo(...P(c.p))
          else
            shape.lineTo(...P(c.p))
        }
        if (m.glass.geometry !== this.quad)
          m.glass.geometry.dispose()
        m.glass.geometry = new THREE.ShapeGeometry(shape, 24)
        m.slabKey = key
      }
      m.glass.position.set(0, 0, z)
      m.glass.scale.set(1, 1, 1)
      u.uUseSdf.value = 0
      m.shadow.visible = false
      m.content.visible = false
    }
    else {
      if (m.glass.geometry !== this.quad) {
        m.glass.geometry.dispose()
        m.glass.geometry = this.quad
        m.slabKey = ''
      }
      m.glass.position.set(cx, cy, z)
      m.glass.scale.set(w, h, 1)
      u.uUseSdf.value = 1
      u.uSize.value.set(w, h)
      const radius = (win.radius ?? RADIUS) * s
      u.uRadius.value = radius

      const pad = 70
      m.shadow.visible = true
      m.shadow.position.set(cx, cy - 18, z - 0.5)
      m.shadow.scale.set(w + pad * 2, h + pad * 2, 1)
      const su = m.shadow.material.uniforms
      su.uSize.value.set(w + pad * 2, h + pad * 2)
      su.uInner.value.set(w, h)
      su.uRadius.value = radius
      su.uColor.value.copy(vec4(theme.shadow))
      su.uOpacity.value = win.opacity * 0.9

      m.content.visible = win.chrome > 0.001
      if (m.content.visible) {
        // Stretching blocks keep their content rasterized at the final size.
        const texWin = win.contentSize ? { ...win, rect: { ...win.rect, ...win.contentSize } } : win
        // Scaled-up windows (mobile gallery) raster at their on-screen size, in
        // quarter steps so an animating scale does not re-raster every frame.
        const cs = Math.min(2.5, Math.ceil(contentScale * Math.max(1, s) * 4) / 4)
        const tex = this.content.get(texWin, theme, cs)
        if (m.content.material.map !== tex) {
          m.content.material.map = tex
          m.content.material.needsUpdate = true
        }
        m.content.material.opacity = win.chrome * win.opacity
        m.content.position.set(cx, cy, z + 0.5)
        m.content.scale.set(w, h, 1)
      }
    }

    u.tBlur.value = blur
    u.uViewport.value.set(this.rtScene.width, this.rtScene.height)
    u.uTint.value.copy(vec4(win.fill ?? (dark ? theme.termTint : theme.tint)))
    u.uEdge.value.copy(vec4(theme.edge))
    u.uHigh.value.copy(vec4(theme.highlight))
    u.uSat.value = theme.name === 'light' ? 1.4 : 1.7
    u.uBright.value = theme.name === 'light' ? 1.04 : 1
    u.uUseBlur.value = glassOn && blur ? 1 : 0
    u.uOpacity.value = win.opacity
  }

  /**
   * Pick the scene target for a frame. Flat desk frames anti-alias every edge
   * in their shaders (SDF windows, shadows, glows, ripples; axis-aligned
   * sprites), so they skip MSAA. Only the opening camera and slab/poly glass
   * (triangulated shapes with hard geometric edges) need it.
   *
   * NOTICE: three.js resolves a multisampled target after every render() call,
   * and this renderer calls render() once per window plus a few layers. At 4K
   * HalfFloat that was 9 full-screen resolves per frame: about 4.6 ms of a
   * synced landing frame vs 1.2 ms without MSAA (8.9 vs 3.1 ms with one glass
   * window), measured on Apple M5 Max / Chrome ANGLE Metal.
   */
  private sceneTarget(state: SceneState) {
    const msaa = !!state.camera || state.windows.some(w => w.opacity > 0.001 && (w.slab || w.poly))
    if (!msaa)
      return this.rtSharp
    if (!this.rtMsaa) {
      const s = this.rtSharp
      this.rtMsaa = new THREE.WebGLRenderTarget(s.width, s.height, { depthBuffer: false, magFilter: THREE.LinearFilter, minFilter: THREE.LinearFilter, samples: 4, type: THREE.HalfFloatType })
    }
    return this.rtMsaa
  }

  private setCamera(cam: Camera | undefined, fit: Fit) {
    const fw = this.width / fit.k
    const fh = this.height / fit.k
    // Frustum centered on the stage center; the fit offset is symmetric.
    const ox = (this.width / 2 - (fit.x + (this.sw * fit.k) / 2)) / fit.k
    const oy = (this.height / 2 - (fit.y + (this.sh * fit.k) / 2)) / fit.k
    if (cam) {
      // Off-axis perspective whose window at the look-at distance is the flat
      // view's frustum scaled by 1 / zoom, so the look-at plane frames alike
      // at any `persp` and only depth changes.
      const c = this.persp
      const dist = eyeDistance(cam, this.sw, this.sh)
      const near = Math.max(1, dist * 0.002)
      const k = near / (dist * cam.zoom)
      c.position.set(...cameraEye(cam, dist))
      c.up.set(0, 1, 0)
      c.lookAt(...cam.at)
      c.projectionMatrix.makePerspective((-fw / 2 + ox) * k, (fw / 2 + ox) * k, (fh / 2 - oy) * k, (-fh / 2 - oy) * k, near, dist + 2e5)
      c.projectionMatrixInverse.copy(c.projectionMatrix).invert()
      this.view = c
      this.eyeDist = dist
      return
    }
    const c = this.camera
    c.left = -fw / 2 + ox
    c.right = fw / 2 + ox
    c.top = fh / 2 - oy
    c.bottom = -fh / 2 - oy
    c.position.set(0, 0, 6000)
    c.up.set(0, 1, 0)
    c.lookAt(0, 0, 0)
    c.zoom = 1
    c.updateProjectionMatrix()
    this.view = c
    this.eyeDist = 6000
  }

  private windowMeshes(id: string) {
    let m = this.windows.get(id)
    if (m)
      return m
    const shadow = new THREE.Mesh(this.quad, worldMat(SHADOW_FRAG, {
      uColor: { value: new THREE.Vector4() },
      uInner: { value: new THREE.Vector2() },
      uOpacity: { value: 1 },
      uRadius: { value: RADIUS },
      uSize: { value: new THREE.Vector2() },
      uSoft: { value: 30 },
    }))
    const glass = new THREE.Mesh<THREE.BufferGeometry, THREE.ShaderMaterial>(this.quad, worldMat(GLASS_FRAG, {
      tBlur: { value: null },
      uBright: { value: 1 },
      uEdge: { value: new THREE.Vector4() },
      uHigh: { value: new THREE.Vector4() },
      uOpacity: { value: 1 },
      uRadius: { value: RADIUS },
      uSat: { value: 1.8 },
      uSize: { value: new THREE.Vector2() },
      uTint: { value: new THREE.Vector4() },
      uUseBlur: { value: 1 },
      uUseSdf: { value: 1 },
      uViewport: { value: new THREE.Vector2() },
    }))
    const content = new THREE.Mesh(this.quad, new THREE.MeshBasicMaterial({ depthTest: false, depthWrite: false, transparent: true }))
    shadow.renderOrder = 0
    glass.renderOrder = 1
    content.renderOrder = 2
    for (const mesh of [shadow, glass, content]) mesh.frustumCulled = false
    const group = new THREE.Group()
    group.add(shadow, glass, content)
    this.scene.add(group)
    m = { content, glass, group, shadow, slabKey: '' }
    this.windows.set(id, m)
    return m
  }
}

/** Eye position: `dist` out along the yaw / pitch direction from `at`. */
function cameraEye(cam: Camera | undefined, dist: number): Vec3 {
  const yaw = cam?.yaw ?? 0
  const pitch = cam?.pitch ?? 0
  const [x, y, z] = cam?.at ?? [0, 0, 0]
  return [x + Math.sin(yaw) * Math.cos(pitch) * dist, y + Math.sin(pitch) * dist, z + Math.cos(yaw) * Math.cos(pitch) * dist]
}

/**
 * Eye distance for the opening camera. At persp = 1 it is a standard
 * perspective (about a 32 degree vertical field at zoom 1); toward 0 it backs
 * off so far that the view is orthographic for any depth the scene uses.
 */
function eyeDistance(cam: Camera, sw: number, sh: number) {
  const standard = 0.85 * Math.hypot(sw, sh)
  return Math.min(4e6, standard / (cam.zoom * Math.max(cam.persp, 1e-4)))
}

function fullscreen(frag: string, uniforms: Record<string, THREE.IUniform>) {
  const mat = new THREE.ShaderMaterial({ depthTest: false, depthWrite: false, fragmentShader: frag, glslVersion: THREE.GLSL3, uniforms, vertexShader: FULL_VERT })
  const mesh = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), mat)
  mesh.frustumCulled = false
  const scene = new THREE.Scene()
  scene.add(mesh)
  return { mat, scene }
}

function worldMat(frag: string, uniforms: Record<string, THREE.IUniform>) {
  return new THREE.ShaderMaterial({ depthTest: false, depthWrite: false, fragmentShader: frag, glslVersion: THREE.GLSL3, transparent: true, uniforms, vertexShader: WORLD_VERT })
}
