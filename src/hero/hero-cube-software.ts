import { cubeTriangles, type CubeVertex as Point } from './hero-cube-mesh';
import { createCubeGlow } from './hero-cube-glow';
import type { CubeLogo } from './ade-logo';
type Rotation = { x: number; y: number; z: number };

// Software lighting for embedded browsers without WebGL. Material opacity is
// always one; the fixed spotlight alone controls the visible falloff.
export function createSoftwareCubeLighting(host: HTMLElement, scene: HTMLElement) {
  const canvas = document.createElement('canvas');
  const context = canvas.getContext('2d');
  if (!context) return null;
  const resolution = 800;
  canvas.width = canvas.height = resolution;
  canvas.className = 'hero-cube-lit-canvas';
  canvas.setAttribute('aria-hidden', 'true');
  const image = context.createImageData(resolution, resolution);
  const depth = new Float32Array(resolution * resolution);
  const noise = Float32Array.from(depth, () => Math.random() - 0.5);
  host.append(canvas);
  host.dataset.lit = 'true';
  const glow = createCubeGlow(host);
  let previous = '';
  let logo: CubeLogo | null = null;
  return {
    setLogo(next: CubeLogo | null) { logo = next; previous = ''; },
    draw(pose: Rotation, size: number, lift: number, opening = 0) {
      const style = getComputedStyle(scene);
      const hex = style.getPropertyValue('--cube-color').trim() || '#101112';
      const intensity = parseFloat(style.getPropertyValue('--case-light') || '0.0625') / 0.0625;
      const offset = (parseFloat(style.getPropertyValue('--case-light-offset')) || 0) * 4;
      const lightY = parseFloat(style.getPropertyValue('--case-light-y') || '-300');
      const lightZ = parseFloat(style.getPropertyValue('--case-light-z') || '900');
      const lightWidth = Math.max(0.01, parseFloat(style.getPropertyValue('--case-light-width') || '1'));
      const lightHex = style.getPropertyValue('--lid-color').trim() || '#FFFFFF';
      const lightColor = [1, 3, 5].map((i) => parseInt(lightHex.slice(i, i + 2), 16));
      const innerLight = parseFloat(style.getPropertyValue('--lid-light') || '0.8');
      const spread = parseFloat(style.getPropertyValue('--lid-glow-spread') || '70');
      glow?.draw(pose, size, lift, opening, innerLight, spread, lightColor);
      const grain = parseFloat(style.getPropertyValue('--case-grain') || '0.12');
      const signature = `${pose.x.toFixed(2)},${pose.y.toFixed(2)},${pose.z.toFixed(2)},${size},${lift},${hex},${intensity},${offset},${lightY},${lightZ},${lightWidth},${grain},${opening.toFixed(2)},${innerLight},${lightHex},${spread}`;
      if (signature === previous) return;
      previous = signature;
      const metal = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
      const [x, y, z] = [pose.x, pose.y, pose.z].map((angle) => (angle * Math.PI) / 180);
      const [sx, cx, sy, cy, sz, cz] = [Math.sin(x), Math.cos(x), Math.sin(y), Math.cos(y), Math.sin(z), Math.cos(z)];
      const rotate = (x: number, y: number, z: number) => {
        const a = x * cz - y * sz,
          b = x * sz + y * cz;
        const c = a * cy + z * sy,
          d = -a * sy + z * cy;
        return [c, b * cx - d * sx, b * sx + d * cx];
      };
      const project = (p: Point) => {
        const [x, y, z] = rotate(p.x * size, (p.y - (p.part === 1 ? opening : 0)) * size, p.z * size);
        const [nx, ny, nz] = rotate(p.nx, p.ny, p.nz);
        const w = 1 / (1600 - z);
        return {
          x,
          y,
          z,
          nx,
          ny,
          nz,
          w,
          px: ((400 + x * 1400 * w) * resolution) / 800,
          py: ((400 + (y + lift) * 1400 * w) * resolution) / 800,
        };
      };
      image.data.fill(0);
      depth.fill(-Infinity);
      for (const triangle of cubeTriangles) {
        const [a, b, c] = triangle.map(project);
        if ((a.nx + b.nx + c.nx) * -a.x + (a.ny + b.ny + c.ny) * -a.y + (a.nz + b.nz + c.nz) * (1600 - a.z) <= 0)
          continue;
        const denom = (b.py - c.py) * (a.px - c.px) + (c.px - b.px) * (a.py - c.py);
        if (Math.abs(denom) < 0.0001) continue;
        const minX = Math.max(0, Math.floor(Math.min(a.px, b.px, c.px))),
          maxX = Math.min(resolution - 1, Math.ceil(Math.max(a.px, b.px, c.px)));
        const minY = Math.max(0, Math.floor(Math.min(a.py, b.py, c.py))),
          maxY = Math.min(resolution - 1, Math.ceil(Math.max(a.py, b.py, c.py)));
        for (let py = minY; py <= maxY; py++)
          for (let px = minX; px <= maxX; px++) {
            const u = ((b.py - c.py) * (px + 0.5 - c.px) + (c.px - b.px) * (py + 0.5 - c.py)) / denom;
            const v = ((c.py - a.py) * (px + 0.5 - c.px) + (a.px - c.px) * (py + 0.5 - c.py)) / denom;
            const w = 1 - u - v;
            if (u < -0.00001 || v < -0.00001 || w < -0.00001) continue;
            const inv = 1 / (u * a.w + v * b.w + w * c.w);
            const ua = u * a.w * inv,
              vb = v * b.w * inv,
              wc = w * c.w * inv;
            const z = ua * a.z + vb * b.z + wc * c.z;
            const pixel = py * resolution + px;
            if (z < depth[pixel]) continue;
            depth[pixel] = z;
            const x = ua * a.x + vb * b.x + wc * c.x,
              y = ua * a.y + vb * b.y + wc * c.y;
            let nx = ua * a.nx + vb * b.nx + wc * c.nx,
              ny = ua * a.ny + vb * b.ny + wc * c.ny,
              nz = ua * a.nz + vb * b.nz + wc * c.nz;
            const normalLength = Math.hypot(nx, ny, nz);
            nx /= normalLength;
            ny /= normalLength;
            nz /= normalLength;
            const lx = offset - x,
              ly = lightY - y,
              lz = lightZ - z;
            const lightLength = Math.hypot(lx, ly, lz);
            const diffuse = Math.max(0, (nx * lx + ny * ly + nz * lz) / lightLength);
            const beam = Math.hypot((x - offset) / (lz * lightWidth), (y - lightY) / lz - 0.46);
            const illumination = Math.exp(-(beam * beam) / 0.04) * intensity;
            const vx = -x,
              vy = -y,
              vz = 1600 - z,
              viewLength = Math.hypot(vx, vy, vz);
            const hx = lx / lightLength + vx / viewLength,
              hy = ly / lightLength + vy / viewLength,
              hz = lz / lightLength + vz / viewLength;
            const specular = Math.pow(Math.max(0, (nx * hx + ny * hy + nz * hz) / Math.hypot(hx, hy, hz)), 16);
            const highlight = (14 * specular + noise[pixel] * grain * 25.5 * diffuse) * illumination;
            const localY = ua * triangle[0].y + vb * triangle[1].y + wc * triangle[2].y;
            const part = triangle[0].part;
            const revealed = Math.min(1, opening / 30) * innerLight;
            const localX = ua * triangle[0].x + vb * triangle[1].x + wc * triangle[2].x;
            const localZ = ua * triangle[0].z + vb * triangle[1].z + wc * triangle[2].z;
            const localNX = ua * triangle[0].nx + vb * triangle[1].nx + wc * triangle[2].nx;
            const localNY = triangle[0].ny;
            const localNZ = ua * triangle[0].nz + vb * triangle[1].nz + wc * triangle[2].nz;
            const dy = 184 - localY + (part === 1 ? opening : 0);
            const distanceSquared = localX * localX + dy * dy + localZ * localZ;
            const incidence = Math.max(0, (-localX * localNX + dy * localNY - localZ * localNZ) /
              Math.sqrt(Math.max(1, distanceSquared) * (localNX * localNX + localNY * localNY + localNZ * localNZ)));
            // Inverse-square falloff and surface orientation from a floor-mounted area light.
            const incoming = 210000 * incidence / Math.max(16000, distanceSquared);
            const emission = 2.4 * Math.exp(-(localX * localX + localZ * localZ) / 6000);
            const interior = part === 2 ? emission : part === 3 || (part === 1 && localNY > 0) ? incoming : 0;
            // Diffuse bounce from the slit washes down the exterior, not over the lid.
            const belowGap = Math.max(0, localY + 192);
            const acrossFace = Math.abs(localNX) >= Math.abs(localNZ) ? localZ : localX;
            const width = 155 + belowGap * 0.3;
            const scatter = part === 0 && Math.abs(localNY) < 0.1 && localY >= -192
              ? 0.5 * Math.exp(-belowGap / (30 + spread * 0.9)) * Math.exp(-((acrossFace / width) ** 4))
              : 0;
            const index = pixel * 4;
            for (let channel = 0; channel < 3; channel++)
              image.data[index + channel] =
                (part >= 2 ? metal[channel] * 0.025 : metal[channel] * (0.025 + illumination * diffuse * 2.3) + highlight)
                + lightColor[channel] * (interior + scatter) * revealed;
            if (logo && part === 1 && localNY < -0.5) {
              const u = localX / logo.width + 0.5, v = localZ / logo.height + 0.5;
              if (u >= 0 && u < 1 && v >= 0 && v < 1) {
                const texel = (Math.floor(v * logo.canvas.height) * logo.canvas.width + Math.floor(u * logo.canvas.width)) * 4;
                const alpha = logo.pixels[texel + 3] / 255;
                const paintLight = Math.min(1, 0.6 + illumination * diffuse * 0.2);
                for (let channel = 0; channel < 3; channel++)
                  image.data[index + channel] = image.data[index + channel] * (1 - alpha) + logo.pixels[texel + channel] * paintLight * alpha;
              }
            }
            image.data[index + 3] = 255;
          }
      }
      context.putImageData(image, 0, 0);
    },
    dispose() {
      canvas.remove();
      glow?.dispose();
      delete host.dataset.lit;
    },
  };
}
