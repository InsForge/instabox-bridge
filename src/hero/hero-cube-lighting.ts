import { cubeFragmentSource } from './cube-material-shader';
import { cubeTriangles } from './hero-cube-mesh';
import { createCubeGlow } from './hero-cube-glow';
import { createSoftwareCubeLighting } from './hero-cube-software';
import type { CubeLogo } from './ade-logo';

type Rotation = { x: number; y: number; z: number };

// One uniform metal material, illuminated by a stationary soft spotlight.
// The geometry has rounded horizontal cross-sections and straight vertical walls.
export function createCubeLighting(host: HTMLElement, scene: HTMLElement) {
  const canvas = document.createElement('canvas');
  const gl = canvas.getContext('webgl', { alpha: true, antialias: true, premultipliedAlpha: false });
  if (!gl) return createSoftwareCubeLighting(host, scene);
  const vertexSource = `
    attribute vec3 position;
    attribute vec3 normal;
    attribute float part;
    uniform float opening;
    varying float surfacePart;
    varying vec3 localPosition;
    varying vec3 localNormal;
    uniform mat3 rotation;
    uniform float size;
    uniform float lift;
    varying vec3 worldPosition;
    varying vec3 worldNormal;
    void main() {
      surfacePart = part;
      localPosition = position;
      localNormal = normal;
      vec3 pLocal = position;
      if (part > 0.5 && part < 1.5) pLocal.y -= opening;
      worldPosition = rotation * pLocal * size;
      worldNormal = rotation * normal;
      vec3 p = worldPosition + vec3(0.0, lift, -200.0);
      float w = 1400.0 - p.z;
      gl_Position = vec4(p.x * 1400.0 / 400.0, -p.y * 1400.0 / 400.0,
        (w - 1400.0) * 0.5, w);
    }
  `;
  const shaders: WebGLShader[] = [];
  const compile = (type: number, source: string) => {
    const shader = gl.createShader(type)!;
    shaders.push(shader);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error('Cube lighting shader failed');
    return shader;
  };
  const program = gl.createProgram()!;
  try {
    gl.attachShader(program, compile(gl.VERTEX_SHADER, vertexSource));
    gl.attachShader(program, compile(gl.FRAGMENT_SHADER, '#define GROUP_LOGOS\n' + cubeFragmentSource));
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error('Cube lighting program failed');
  } catch {
    shaders.forEach((shader) => gl.deleteShader(shader));
    gl.deleteProgram(program);
    return createSoftwareCubeLighting(host, scene);
  }
  const vertices = cubeTriangles.flatMap((triangle) =>
    triangle.flatMap((p) => [p.x, p.y, p.z, p.nx, p.ny, p.nz, p.part]),
  );
  const buffer = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(vertices), gl.STATIC_DRAW);
  gl.useProgram(program);
  for (const [name, offset] of [
    ['position', 0],
    ['normal', 12],
    ['part', 24],
  ] as const) {
    const location = gl.getAttribLocation(program, name);
    gl.enableVertexAttribArray(location);
    gl.vertexAttribPointer(location, name === 'part' ? 1 : 3, gl.FLOAT, false, 28, offset);
  }
  const uniforms = Object.fromEntries(
    ['rotation', 'size', 'lift', 'metal', 'intensity', 'lightPosition', 'lightWidth', 'grain', 'opening', 'innerLight', 'interiorColor', 'scatterSpread', 'logoTexture', 'logoDimensions', 'logoOpacity'].map(
      (name) => [name, gl.getUniformLocation(program, name)],
    ),
  );
  const texture = gl.createTexture();
  gl.activeTexture(gl.TEXTURE0);
  gl.bindTexture(gl.TEXTURE_2D, texture);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array(4));
  gl.uniform1i(uniforms.logoTexture, 0);
  gl.uniform1f(uniforms.logoOpacity, 1);
  gl.enable(gl.DEPTH_TEST);
  canvas.className = 'hero-cube-lit-canvas';
  canvas.setAttribute('aria-hidden', 'true');
  host.append(canvas);
  host.dataset.lit = 'true';
  const glow = createCubeGlow(host);
  const lost = (event: Event) => {
    event.preventDefault();
    delete host.dataset.lit;
    canvas.style.visibility = 'hidden';
  };
  canvas.addEventListener('webglcontextlost', lost);
  return {
    setLogo(logo: CubeLogo | null) {
      gl.bindTexture(gl.TEXTURE_2D, texture);
      if (logo) gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, logo.canvas);
      gl.uniform2f(uniforms.logoDimensions, logo?.width ?? 0, logo?.height ?? 0);
    },
    draw(pose: Rotation, size: number, lift: number, opening = 0) {
      // Match the small desktop render instead of allocating a landing-page-sized buffer.
      const displayWidth = canvas.getBoundingClientRect().width || 200;
      const resolution = Math.max(200, Math.round(displayWidth * Math.min(window.devicePixelRatio || 1, 2)));
      if (canvas.width !== resolution) canvas.width = canvas.height = resolution;
      gl.viewport(0, 0, resolution, resolution);
      const [x, y, z] = [pose.x, pose.y, pose.z].map((angle) => (angle * Math.PI) / 180);
      const [sx, cx, sy, cy, sz, cz] = [Math.sin(x), Math.cos(x), Math.sin(y), Math.cos(y), Math.sin(z), Math.cos(z)];
      gl.uniformMatrix3fv(
        uniforms.rotation,
        false,
        new Float32Array([
          cy * cz,
          cx * sz + sx * sy * cz,
          sx * sz - cx * sy * cz,
          -cy * sz,
          cx * cz - sx * sy * sz,
          sx * cz + cx * sy * sz,
          sy,
          -sx * cy,
          cx * cy,
        ]),
      );
      const style = getComputedStyle(scene);
      const lightHex = style.getPropertyValue('--lid-color').trim() || '#FFFFFF';
      const lightColor = [1, 3, 5].map((i) => parseInt(lightHex.slice(i, i + 2), 16));
      gl.uniform3f(uniforms.interiorColor, lightColor[0] / 255, lightColor[1] / 255, lightColor[2] / 255);
      const innerLight = parseFloat(style.getPropertyValue('--lid-light') || '0.8');
      const spread = parseFloat(style.getPropertyValue('--lid-glow-spread') || '70');
      glow?.draw(pose, size, lift, opening, innerLight, spread, lightColor);
      gl.uniform1f(uniforms.opening, opening);
      gl.uniform1f(uniforms.innerLight, innerLight);
      gl.uniform1f(uniforms.scatterSpread, spread);
      const hex = style.getPropertyValue('--cube-color').trim() || '#101112';
      gl.uniform3f(
        uniforms.metal,
        parseInt(hex.slice(1, 3), 16) / 255,
        parseInt(hex.slice(3, 5), 16) / 255,
        parseInt(hex.slice(5, 7), 16) / 255,
      );
      gl.uniform1f(uniforms.intensity, parseFloat(style.getPropertyValue('--case-light') || '0.0625') / 0.0625);
      gl.uniform1f(uniforms.lightWidth, Math.max(0.01, parseFloat(style.getPropertyValue('--case-light-width') || '1')));
      gl.uniform3f(
        uniforms.lightPosition,
        (parseFloat(style.getPropertyValue('--case-light-offset')) || 0) * 4,
        parseFloat(style.getPropertyValue('--case-light-y') || '-300'),
        parseFloat(style.getPropertyValue('--case-light-z') || '900'),
      );
      gl.uniform1f(uniforms.grain, parseFloat(style.getPropertyValue('--case-grain') || '0.12'));
      gl.uniform1f(uniforms.size, size);
      gl.uniform1f(uniforms.lift, lift);
      gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
      gl.drawArrays(gl.TRIANGLES, 0, vertices.length / 7);
    },
    dispose() {
      delete host.dataset.lit;
      canvas.removeEventListener('webglcontextlost', lost);
      canvas.remove();
      glow?.dispose();
      gl.deleteBuffer(buffer);
      gl.deleteTexture(texture);
      gl.deleteProgram(program);
      shaders.forEach((shader) => gl.deleteShader(shader));
      gl.getExtension('WEBGL_lose_context')?.loseContext();
    },
  };
}
