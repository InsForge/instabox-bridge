// Upward scattering confined to the opening. Opaque geometry masks the beam.
export function createCubeGlow(host: HTMLElement) {
  const canvas = document.createElement('canvas');
  const context = canvas.getContext('2d');
  if (!context || typeof context.createRadialGradient !== 'function') return null;
  canvas.width = canvas.height = 800;
  canvas.className = 'hero-cube-lit-canvas hero-cube-glow-canvas';
  canvas.setAttribute('aria-hidden', 'true');
  host.prepend(canvas);
  let previous = '';
  return {
    draw(
      pose: { x: number; y: number; z: number },
      size: number,
      lift: number,
      opening: number,
      strength: number,
      spread: number,
      color = [255, 255, 255],
    ) {
      const signature = `${pose.x},${pose.y},${pose.z},${size},${lift},${opening.toFixed(2)},${strength},${spread},${color.join(",")}`;
      if (signature === previous) return;
      previous = signature;
      context.clearRect(0, 0, 800, 800);
      if (opening < 0.1 || strength <= 0) return;
      const [x, y, z] = [pose.x, pose.y, pose.z].map((v) => (v * Math.PI) / 180);
      const project = (px: number, py: number, pz: number) => {
        const a = px * Math.cos(z) - py * Math.sin(z),
          b = px * Math.sin(z) + py * Math.cos(z);
        const c = a * Math.cos(y) + pz * Math.sin(y),
          d = -a * Math.sin(y) + pz * Math.cos(y);
        const wy = (b * Math.cos(x) - d * Math.sin(x)) * size;
        const wz = (b * Math.sin(x) + d * Math.cos(x)) * size;
        const scale = 1400 / (1600 - wz);
        return [400 + c * size * scale, 400 + (wy + lift) * scale];
      };
      const amount = Math.min(1, opening / 30) * strength;
      // Nested cones rise from the floor source and terminate at the lid underside.
      // No screen-space halo surrounds the lid or illuminates its upper surface.
      for (let layer = 0; layer < 24; layer++) {
        const radius = 55 + spread * 0.35 + layer * 3.5;
        const upperRadius = radius * (384 + opening) / 384;
        const points: number[][] = [];
        for (let i = 0; i < 64; i++) {
          const angle = i * Math.PI / 32;
          points.push(project(Math.cos(angle) * radius, -192, Math.sin(angle) * radius));
          points.push(project(Math.cos(angle) * upperRadius, -192 - opening, Math.sin(angle) * upperRadius));
        }
        // Convex hull of the projected cone keeps scattering within its silhouette.
        points.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
        const cross = (a: number[], b: number[], c: number[]) =>
          (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        const half = (ordered: number[][]) => {
          const hull: number[][] = [];
          for (const point of ordered) {
            while (hull.length > 1 && cross(hull[hull.length - 2], hull[hull.length - 1], point) <= 0) hull.pop();
            hull.push(point);
          }
          return hull.slice(0, -1);
        };
        const hull = [...half(points), ...half([...points].reverse())];
        context.fillStyle = `rgba(${color.join(",")},${Math.min(0.035, amount * 0.018)})`;
        context.beginPath();
        hull.forEach(([px, py], j) => j ? context.lineTo(px, py) : context.moveTo(px, py));
        context.closePath();
        context.fill();
      }
    },
    dispose() {
      canvas.remove();
    },
  };
}
