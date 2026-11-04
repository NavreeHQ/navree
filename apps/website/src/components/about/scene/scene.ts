/**
 * The live about picture in three.js: the still picture comes to life.
 *
 * Two draws make a frame:
 * - the picture itself on a mesh bent to its depth (`depth.ts`), so the
 *   chair, the father, his children, the desk and the room each stand at
 *   their own distance and shift a little against each other as the camera
 *   moves;
 * - every glyph in one instanced draw, added as light: the digits that
 *   rain down and gather into the picture, then shimmer and stream through
 *   the family; the rain at the edges; the code typed on the screen; a glow
 *   that flashes with each pulse. No post-processing: each glyph's glow is
 *   part of its cell in the atlas.
 *
 * `render(t)` is a pure function of time (plus an optional camera offset
 * for the pointer and scroll), so the same `t` always draws the same frame.
 * This module is loaded only on `/about/`, after the page has painted.
 */
import {
  BufferAttribute,
  BufferGeometry,
  CanvasTexture,
  CustomBlending,
  InstancedBufferAttribute,
  InstancedBufferGeometry,
  LinearFilter,
  LinearMipmapLinearFilter,
  Mesh,
  OneFactor,
  PerspectiveCamera,
  Scene as Stage,
  ShaderMaterial,
  Texture,
  Vector3,
  WebGLRenderer,
  type IUniform,
} from "three";

import { CODE, IMAGE } from "../picture.ts";
import { drawAtlas } from "./atlas.ts";
import { buildDepth } from "./depth.ts";
import { buildLayout, type Tier } from "./layout.ts";
import { glyphFragment, glyphVertex, pictureFragment, pictureVertex } from "./shaders.ts";
import { CENTER, DEPTH, FOV, toWorld } from "./space.ts";
import { BEATS, FALL, PULSE, SLOW_FOR, TYPING, cameraAt, levels, progress } from "./timeline.ts";
import { caretAt, roundTime } from "./typing.ts";

export type SceneOptions = {
  canvas: HTMLCanvasElement;
  /** The still picture, already decoded: the scene is built from it. */
  image: HTMLImageElement;
  tier: Tier;
  /** CSS font family for the digits and the code. */
  font: string;
  width: number;
  height: number;
  pixelRatio: number;
  antialias: boolean;
};

export type LiveScene = {
  /** Draws the frame at `t` seconds; `view` moves the camera a little (picture pixels) for the pointer and scroll. */
  render(t: number, view?: { x: number; y: number }): void;
  resize(width: number, height: number, pixelRatio: number): void;
  /** Steps quality down: 0 draws every glyph, higher steps draw fewer. Returns false when there is no lower step. */
  setStep(step: number): boolean;
  readonly glyphs: number;
  dispose(): void;
};

/** Lets the browser breathe between the steps of setting up, so none of them is a long task. */
const yieldToMain = () =>
  new Promise<void>((resolve) => {
    const scheduler = (globalThis as { scheduler?: { yield?: () => Promise<void> } }).scheduler;
    if (scheduler?.yield) void scheduler.yield().then(resolve);
    else setTimeout(resolve, 0);
  });

export async function createScene(opts: SceneOptions): Promise<LiveScene | null> {
  const atlasCanvas = drawAtlas(opts.font);
  if (!atlasCanvas) return null;
  await yieldToMain();
  const depth = buildDepth(4);
  await yieldToMain();
  const layout = buildLayout(opts.tier, opts.image, depth);
  await yieldToMain();

  let renderer: WebGLRenderer;
  try {
    renderer = new WebGLRenderer({
      canvas: opts.canvas,
      antialias: opts.antialias,
      alpha: false,
      powerPreference: "low-power",
      preserveDrawingBuffer: false,
      stencil: false,
      depth: false,
    });
  } catch {
    return null;
  }
  renderer.setClearColor(0x040605, 1);
  renderer.setPixelRatio(opts.pixelRatio);
  renderer.setSize(opts.width, opts.height, false);

  const scene = new Stage();
  const camera = new PerspectiveCamera(FOV, opts.width / opts.height, 50, 8000);

  // ---- the picture, bent to its depth
  // Its pixels at their real size: an <img> with a srcset reports a size
  // corrected for density, which isn't the size of the file it holds. The
  // file is in the cache already, so this is no new download.
  let bitmap: ImageBitmap;
  try {
    const blob = await (await fetch(opts.image.currentSrc || opts.image.src)).blob();
    bitmap = await createImageBitmap(blob, { imageOrientation: "flipY" });
  } catch {
    renderer.dispose();
    return null;
  }
  const image = new Texture(bitmap);
  image.flipY = false;
  image.generateMipmaps = true;
  image.minFilter = LinearMipmapLinearFilter;
  image.magFilter = LinearFilter;
  image.needsUpdate = true;
  const mask = new CanvasTexture(layout.mask);
  mask.minFilter = LinearFilter;
  mask.generateMipmaps = false;

  const nx = opts.tier.mesh;
  const ny = Math.round((nx * IMAGE.height) / IMAGE.width);
  const positions = new Float32Array((nx + 1) * (ny + 1) * 3);
  const uvs = new Float32Array((nx + 1) * (ny + 1) * 2);
  for (let j = 0; j <= ny; j++) {
    for (let i = 0; i <= nx; i++) {
      const v = j * (nx + 1) + i;
      const x = (i / nx) * IMAGE.width;
      const y = (j / ny) * IMAGE.height;
      positions.set(toWorld(x, y, depth.at(x, y)), v * 3);
      uvs.set([i / nx, 1 - j / ny], v * 2);
    }
  }
  const index: number[] = [];
  for (let j = 0; j < ny; j++) {
    for (let i = 0; i < nx; i++) {
      const a = j * (nx + 1) + i;
      index.push(a, a + nx + 1, a + 1, a + 1, a + nx + 1, a + nx + 2);
    }
  }
  const pictureGeometry = new BufferGeometry();
  pictureGeometry.setAttribute("position", new BufferAttribute(positions, 3));
  pictureGeometry.setAttribute("depthPosition", new BufferAttribute(positions, 3));
  pictureGeometry.setAttribute("uv", new BufferAttribute(uvs, 2));
  pictureGeometry.setIndex(index);
  const lightPos = new Vector3(...layout.light);
  const pictureUniforms: Record<string, IUniform> = {
    uImage: { value: image },
    uMask: { value: mask },
    uTime: { value: 0 },
    uScreen: { value: 0 },
    uLight: { value: 0 },
    uReveal: { value: 0 },
    uLightPos: { value: lightPos },
    uPulseAge: { value: 1e3 },
    uPulse: { value: [PULSE.speed, PULSE.width, PULSE.life] },
  };
  const pictureMaterial = new ShaderMaterial({
    vertexShader: pictureVertex,
    fragmentShader: pictureFragment,
    uniforms: pictureUniforms,
    depthTest: false,
    depthWrite: false,
  });
  const pictureMesh = new Mesh(pictureGeometry, pictureMaterial);
  pictureMesh.frustumCulled = false;
  pictureMesh.renderOrder = 0;
  scene.add(pictureMesh);

  // ---- the glyphs: one instanced draw
  const atlas = new CanvasTexture(atlasCanvas);
  atlas.generateMipmaps = true;
  atlas.minFilter = LinearMipmapLinearFilter;
  atlas.magFilter = LinearFilter;
  const glyphGeometry = new InstancedBufferGeometry();
  glyphGeometry.setAttribute("position", new BufferAttribute(new Float32Array([-0.5, -0.5, 0, 0.5, -0.5, 0, 0.5, 0.5, 0, -0.5, 0.5, 0]), 3));
  glyphGeometry.setAttribute("uv", new BufferAttribute(new Float32Array([0, 0, 1, 0, 1, 1, 0, 1]), 2));
  glyphGeometry.setIndex([0, 1, 2, 0, 2, 3]);
  glyphGeometry.setAttribute("iA", new InstancedBufferAttribute(layout.a, 4));
  glyphGeometry.setAttribute("iB", new InstancedBufferAttribute(layout.b, 4));
  glyphGeometry.setAttribute("iC", new InstancedBufferAttribute(layout.c, 4));
  glyphGeometry.setAttribute("iD", new InstancedBufferAttribute(layout.d, 4));
  glyphGeometry.instanceCount = layout.count;
  const glyphUniforms: Record<string, IUniform> = {
    uAtlas: { value: atlas },
    uTime: { value: 0 },
    uScreen: { value: 0 },
    uLight: { value: 0 },
    uRain: { value: 1 },
    uLightPos: { value: lightPos },
    uPulseAge: { value: 1e3 },
    uPulseIndex: { value: 0 },
    uRound: { value: -1 },
    uCodeShown: { value: 1 },
    uCaret: { value: [0, 0, 0, 0] },
    uFall: { value: [FALL.speed, FALL.slow, SLOW_FOR] },
    uPulse: { value: [PULSE.speed, PULSE.width, PULSE.life] },
    uCenter: { value: [CENTER.x, CENTER.y] },
  };
  const glyphMaterial = new ShaderMaterial({
    vertexShader: glyphVertex,
    fragmentShader: glyphFragment,
    uniforms: glyphUniforms,
    transparent: true,
    depthWrite: false,
    depthTest: false,
    blending: CustomBlending,
    blendSrc: OneFactor,
    blendDst: OneFactor,
  });
  const glyphMesh = new Mesh(glyphGeometry, glyphMaterial);
  glyphMesh.frustumCulled = false;
  glyphMesh.renderOrder = 1;
  scene.add(glyphMesh);

  // Compile the shaders without blocking, where the browser can do it in the background.
  await yieldToMain();
  try {
    await renderer.compileAsync(scene, camera);
  } catch {
    renderer.compile(scene, camera);
  }
  await yieldToMain();

  let disposed = false;
  return {
    get glyphs() {
      return glyphGeometry.instanceCount;
    },
    render(t, view) {
      if (disposed) return;
      const lv = levels(t);
      const sincePulse = t - BEATS.pulse;
      const pulseIndex = sincePulse < 0 ? -1 : Math.floor(sincePulse / BEATS.pulseEvery);
      const pulseAge = sincePulse < 0 ? 1e3 : sincePulse - pulseIndex * BEATS.pulseEvery;

      // Typing: where this round is, whether the code shows, and the caret.
      const u = roundTime(t, layout.typing);
      const holdEnd = layout.typing.done + TYPING.hold;
      const shown = u < holdEnd ? 1 : 1 - progress(u, holdEnd, holdEnd + TYPING.clear);
      const c = caretAt(t, layout.typing, CODE);
      glyphUniforms.uCaret.value = [...layout.caretAt(c.line, c.col), c.on ? 1 : 0];

      for (const uniforms of [glyphUniforms, pictureUniforms]) {
        uniforms.uTime.value = t;
        uniforms.uScreen.value = lv.screen;
        uniforms.uLight.value = lv.light;
        uniforms.uPulseAge.value = pulseAge;
      }
      glyphUniforms.uRain.value = lv.rain;
      glyphUniforms.uPulseIndex.value = pulseIndex + 1;
      glyphUniforms.uRound.value = u;
      glyphUniforms.uCodeShown.value = shown;

      const cam = cameraAt(t);
      const x = cam.x + (view?.x ?? 0);
      const y = cam.y + (view?.y ?? 0);
      camera.position.set(CENTER.x + x, -CENTER.y + y, DEPTH * (1 + cam.dolly));
      camera.lookAt(CENTER.x + x * 0.2, -CENTER.y + y * 0.2, -260);
      renderer.render(scene, camera);
    },
    resize(width, height, pixelRatio) {
      renderer.setPixelRatio(pixelRatio);
      renderer.setSize(width, height, false);
      camera.aspect = width / height;
      camera.updateProjectionMatrix();
    },
    setStep(step) {
      const count = layout.steps[step];
      if (count === undefined) return false;
      glyphGeometry.instanceCount = count;
      return true;
    },
    dispose() {
      disposed = true;
      scene.clear();
      glyphGeometry.dispose();
      glyphMaterial.dispose();
      pictureGeometry.dispose();
      pictureMaterial.dispose();
      atlas.dispose();
      image.dispose();
      bitmap.close();
      mask.dispose();
      atlasCanvas.width = atlasCanvas.height = 0;
      layout.mask.width = layout.mask.height = 0;
      renderer.dispose();
      renderer.forceContextLoss();
    },
  };
}
