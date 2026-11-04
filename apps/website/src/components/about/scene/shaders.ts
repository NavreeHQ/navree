/**
 * The live picture's shaders. Both work out everything from the time alone,
 * so a frame is a pure function of time:
 *
 * - the picture: a mesh carrying the picture, bent to its depth. It appears
 *   column by column as the opening's digits land on it, dims its screen
 *   for the typed code, hides its own edge rain, brightens as the screen
 *   comes on, and lets each pulse of light run through it;
 * - the glyphs: every digit, character and glow is one instance of a small
 *   quad facing the camera; the vertex shader decides where it is, which
 *   glyph it shows and how bright it is. Groups match `GROUP` in
 *   `layout.ts`.
 */
import { IMAGE } from "../picture.ts";
import { GRID, ROWS } from "./atlas.ts";
import { GROUP } from "./layout.ts";
import { COLUMN, DEPTH, LAND } from "./space.ts";

const f = (n: number) => (Number.isInteger(n) ? `${n}.0` : `${n}`);

/** GLSL shared by both: the hash, and when picture pixel `p` lands in the opening. */
const common = /* glsl */ `
// A hash without sine, steady at any size on any GPU (Dave Hoskins).
float hash(float p) {
  p = fract(p * 0.1031);
  p *= p + 33.33;
  p *= p + p;
  return fract(p);
}
float landAt(vec2 p) {
  float column = floor(p.x / ${f(COLUMN)});
  float spread = min(1.0, abs(p.x - ${f(LAND.middle)}) / ${f(LAND.reach)});
  return ${f(LAND.from)} + ${f(LAND.spread)} * hash(column * 1.37 + 0.21) + ${f(LAND.edges)} * spread + ${f(LAND.fall)} * (p.y / ${f(IMAGE.height)});
}
// The pulse: a ring of light leaving the screen.
float ringAt(vec3 world, vec3 light, float age, vec3 pulse) {
  float r = age * pulse.x;
  float d = distance(world, light);
  return exp(-pow((d - r) / pulse.y, 2.0)) * (1.0 - smoothstep(pulse.z * 0.5, pulse.z, age));
}
`;

export const pictureVertex = /* glsl */ `
attribute vec3 depthPosition;
varying vec2 vUv;
varying vec3 vWorld;
void main() {
  vUv = uv;
  vWorld = depthPosition;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(depthPosition, 1.0);
}
`;

export const pictureFragment = /* glsl */ `
precision highp float;
uniform sampler2D uImage;
uniform sampler2D uMask;
uniform float uTime;
uniform float uScreen;
uniform float uLight;
uniform float uReveal;
uniform vec3 uLightPos;
uniform float uPulseAge;
uniform vec3 uPulse;
varying vec2 vUv;
varying vec3 vWorld;
${common}
void main() {
  vec3 c = texture2D(uImage, vUv).rgb;
  vec2 m = texture2D(uMask, vUv).rg;
  vec2 p = vec2(vUv.x, 1.0 - vUv.y) * vec2(${f(IMAGE.width)}, ${f(IMAGE.height)});
  // The picture's own edge rain gives way to the live rain.
  c *= 1.0 - m.g;
  // The screen's glass: dark until the screen comes on, then its own text dims under the typed code.
  c *= mix(1.0, 0.07 * uScreen, m.r);
  // Lit by the screen: darker until its light reaches the family.
  c *= mix(0.42, 1.0, uLight);
  // The opening: each column appears as its digits land.
  float land = landAt(p);
  c *= mix(smoothstep(land - 0.08, land + 0.55, uTime), 1.0, uReveal);
  // The pulse brightens the picture as it passes, a little towards mint.
  float ring = ringAt(vWorld, uLightPos, uPulseAge, uPulse) * (1.0 - m.r);
  c *= 1.0 + 0.5 * ring;
  gl_FragColor = vec4(c, 1.0);
}
`;

export const glyphVertex = /* glsl */ `
precision highp float;

attribute vec4 iA; // world x, y, z, quad size
attribute vec4 iB; // atlas cell (-1 a changing digit), brightness, tint, figure (picture digits)
attribute vec4 iC; // seed, group, glow, a value of the group's own
attribute vec4 iD; // picture x, y, column, row

uniform float uTime;
uniform float uScreen;
uniform float uLight;
uniform float uRain;
uniform vec3 uLightPos;
uniform float uPulseAge;
uniform float uPulseIndex;
uniform float uRound;
uniform float uCodeShown;
uniform vec4 uCaret;
uniform vec3 uFall; // speed, slowing distance, slowing time
uniform vec3 uPulse; // speed, width, life
uniform vec2 uCenter;

varying vec2 vUv;
varying vec3 vColor;
varying vec3 vMix;

const vec3 GREEN = vec3(0.239, 1.0, 0.561);
const vec3 MINT = vec3(0.851, 1.0, 0.91);
${common}

void main() {
  vec3 pos = iA.xyz;
  float size = iA.w;
  float cell = iB.x;
  float bright = iB.y;
  float tint = iB.z;
  float seed = iC.x;
  float group = iC.y;
  float glow = iC.z;
  float extra = iC.w;
  float t = uTime;
  float k = (${f(DEPTH)} - pos.z) / ${f(DEPTH)};

  vec2 aspect = vec2(1.0);
  float stretch = 1.0;
  float swap = 0.0;
  float rate = 0.05 + 0.2 * hash(seed * 917.0 + 3.0);
  vec3 mixw = vec3(1.0, glow, 0.0);

  if (group == ${f(GROUP.picture)} || group == ${f(GROUP.dust)}) {
    // The opening: fall from above at full speed, slow down into place, glow as it lands.
    float land = landAt(iD.xy) + (hash(seed * 53.0) - 0.5) * 0.08 + (group == ${f(GROUP.dust)} ? 0.06 : 0.0);
    float tau = land - t;
    float fall = 0.0;
    float speed = 0.0;
    if (tau > 0.0) {
      if (tau < uFall.z) {
        float u = tau / uFall.z;
        fall = uFall.y * u * u * u;
        speed = 3.0 * uFall.y * u * u / uFall.z;
      } else {
        fall = uFall.y + uFall.x * (tau - uFall.z);
        speed = uFall.x;
      }
    }
    float moving = speed / uFall.x;
    pos.y += fall * k;
    stretch = 1.0 + 0.6 * moving;
    float since = max(0.0, t - land);
    float landed = step(0.0, t - land);
    float settle = exp(-since * 2.4);
    rate = mix(rate, 14.0, step(0.04, moving));

    if (group == ${f(GROUP.dust)}) {
      bright *= (1.0 - landed) * (0.3 + 0.35 * moving) + landed * 0.5 * settle;
      tint = mix(tint, 0.6, moving);
      mixw = vec3(0.8, 0.0, 0.0);
    } else {
      float figure = iB.w;
      // Over the family: digits that glint in and out, slowly...
      float glint = smoothstep(0.86, 1.0, 0.5 + 0.5 * sin(t * (0.25 + 0.5 * hash(seed * 31.0)) + seed * 60.0));
      float rest = figure * glint * 0.55;
      // ...and a drop of light streaming down some columns.
      float column = iD.z;
      if (figure > 0.5 && hash(column * 1.37 + 0.21) > 0.45) {
        float period = 8.0 + 10.0 * hash(column * 2.11 + 1.7);
        float rows = 5.0 + 5.0 * hash(column * 3.71 + 0.9);
        float len = 5.0 + 8.0 * hash(column * 5.3 + 2.3);
        float head = mod(t + period * hash(column * 7.9 + 4.1), period) * rows - 4.0;
        float d = head - iD.w;
        if (d >= 0.0 && d < len) {
          float trail = pow(1.0 - d / len, 1.6);
          rest += trail * 0.7;
          if (d < 1.0) tint = mix(tint, 1.0, 0.7);
          swap += floor(head);
        }
      }
      // The pulse: the digits it passes light up and change.
      float ring = ringAt(iA.xyz, uLightPos, uPulseAge, uPulse);
      rest += ring * (0.35 + 0.6 * figure);
      tint = mix(tint, 1.0, ring * 0.6);
      swap += step(distance(iA.xyz, uLightPos), uPulseAge * uPulse.x) * uPulseIndex * 3.0;
      rest *= uLight;
      // Falling digits are the rain's bright heads; on landing they glow, then settle to rest.
      bright *= mix(landed * (0.8 * settle + rest), 0.72, moving);
      tint = mix(tint, 0.45, moving);
    }
  } else if (group == ${f(GROUP.rain)}) {
    // The edge rain: a drop down every column, its head pale, its trail fading, over faint digits.
    float column = iD.z;
    float period = 3.2 + 4.0 * hash(column * 1.93 + 0.37);
    float rows = 5.0 + 6.0 * hash(column * 2.71 + 0.11);
    float len = 7.0 + 12.0 * hash(column * 4.13 + 0.53);
    float head = mod(t + period * hash(column * 6.7 + 2.9), period) * rows - 3.0;
    float d = head - iD.w;
    float trail = (d >= 0.0 && d < len) ? pow(1.0 - d / len, 1.4) : 0.0;
    if (d >= 0.0 && d < 1.0) tint = 0.8;
    // iB.w: 1 for the edges' rain, which stays; 0 for the opening's rain across the room, which goes as the picture forms.
    float stay = iB.w + (1.0 - iB.w) * (1.0 - smoothstep(1.2, 2.6, t));
    bright *= (0.28 * iB.w + 0.72 * trail + 0.28 * trail * (1.0 - iB.w)) * uRain * stay;
    swap += floor(head * 0.5);
    rate = 0.5;
  } else if (group == ${f(GROUP.bokeh)}) {
    // Out-of-focus digits drifting down close to the camera.
    float y = mod(iD.y + extra * t + 80.0, 928.0) - 80.0;
    pos.y = -(uCenter.y + (y - uCenter.y) * k);
    bright *= smoothstep(-80.0, 60.0, y) * (1.0 - smoothstep(700.0, 830.0, y));
    mixw = vec3(0.0, 0.15, 1.0);
    rate = 0.1;
  } else if (group == ${f(GROUP.glow)}) {
    aspect = vec2(extra, 1.0);
    float flash = exp(-uPulseAge * 2.0) * step(0.0, uPulseAge);
    bright *= uScreen * (0.6 + 0.6 * flash);
    mixw = vec3(1.0, 0.0, 0.0);
  } else if (group == ${f(GROUP.code)}) {
    // A character shows once typed; the title and line numbers always, once the screen is on.
    float since = uRound - extra;
    float shown = extra < -100.0 ? 1.0 : step(0.0, since) * uCodeShown;
    float pop = extra < -100.0 ? 0.0 : exp(-max(since, 0.0) * 9.0) * step(0.0, since);
    bright *= shown * (1.0 + 0.8 * pop) * uScreen * uScreen;
    tint = mix(tint, 1.0, pop);
    mixw = vec3(1.0, glow * 0.6, 0.0);
  } else if (group == ${f(GROUP.caret)}) {
    pos = uCaret.xyz;
    bright *= uCaret.w * uScreen;
    mixw = vec3(1.0, 0.5, 0.0);
  }

  if (cell < 0.0) cell = step(0.5, hash(seed * 311.0 + floor(t * rate + seed * 17.0) * 7.31 + swap * 13.7));

  float lum = clamp(bright, 0.0, 1.6);
  vColor = mix(GREEN, MINT, clamp(tint, 0.0, 1.0)) * lum;
  vMix = mixw;

  // The quad: the cell of the atlas, facing the camera, stretched while falling (anchored at its head).
  vec2 corner = position.xy;
  float cx = mod(cell, ${f(GRID)});
  float cy = floor(cell / ${f(GRID)});
  vUv = vec2((cx + uv.x) / ${f(GRID)}, 1.0 - (cy + 1.0 - uv.y) / ${f(ROWS)});
  vec4 mv = modelViewMatrix * vec4(pos, 1.0);
  mv.xy += corner * size * aspect * vec2(1.0, stretch);
  mv.y += (stretch - 1.0) * size * 0.32;
  if (lum < 0.003) mv = vec4(0.0, 0.0, 1.0e5, 1.0);
  gl_Position = projectionMatrix * mv;
}
`;

export const glyphFragment = /* glsl */ `
precision mediump float;
uniform sampler2D uAtlas;
varying vec2 vUv;
varying vec3 vColor;
varying vec3 vMix;
void main() {
  vec3 m = texture2D(uAtlas, vUv).rgb;
  gl_FragColor = vec4(vColor * dot(m, vMix), 1.0);
}
`;
