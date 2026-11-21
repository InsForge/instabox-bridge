export type CubeVertex = { x: number; y: number; z: number; nx: number; ny: number; nz: number; part: number };
// part 0: body and rim; 1: moving lid; 2: floor emitter; 3: inner walls.
const outline: CubeVertex[] = [];
for (const [cx, cz, start] of [
  [172, 172, 0],
  [-172, 172, 90],
  [-172, -172, 180],
  [172, -172, 270],
]) {
  for (let i = 0; i <= 32; i++) {
    const angle = ((start + (i * 90) / 32) * Math.PI) / 180;
    outline.push({
      x: cx + 28 * Math.cos(angle),
      y: 0,
      z: cz + 28 * Math.sin(angle),
      nx: Math.cos(angle),
      ny: 0,
      nz: Math.sin(angle),
      part: 0,
    });
  }
}
export const cubeTriangles: CubeVertex[][] = [];
outline.forEach((a, i) => {
  const b = outline[(i + 1) % outline.length];
  const wall = (top: number, bottom: number, part: number) => {
    const at = { ...a, y: top, part },
      ab = { ...a, y: bottom, part };
    const bt = { ...b, y: top, part },
      bb = { ...b, y: bottom, part };
    cubeTriangles.push([at, ab, bb], [at, bb, bt]);
  };
  wall(-192, 200, 0);
  wall(-200, -192, 1);
  // Smooth radial interior normals remove visible wall-to-corner shading turns.
  const inner = (p: CubeVertex, y: number): CubeVertex => ({
    ...p, x: p.x * 0.94, z: p.z * 0.94, y,
    nx: -p.x / Math.hypot(p.x, p.z), nz: -p.z / Math.hypot(p.x, p.z), part: 3,
  });
  const at = inner(a, -192), ab = inner(a, 192);
  const bt = inner(b, -192), bb = inner(b, 192);
  cubeTriangles.push([at, bb, ab], [at, bt, bb]);
  const rim = (p: CubeVertex): CubeVertex => ({ ...p, y: -192, nx: 0, ny: -1, nz: 0, part: 0 });
  cubeTriangles.push([rim(a), rim(b), rim(bt)], [rim(a), rim(bt), rim(at)]);
  cubeTriangles.push([
    { x: 0, y: 192, z: 0, nx: 0, ny: -1, nz: 0, part: 2 },
    { ...ab, nx: 0, ny: -1, nz: 0, part: 2 },
    { ...bb, nx: 0, ny: -1, nz: 0, part: 2 },
  ]);
  for (const [y, ny, part] of [
    [-200, -1, 1],
    [-192, 1, 1],

    [200, 1, 0],
  ]) {
    cubeTriangles.push([
      { x: 0, y, z: 0, nx: 0, ny, nz: 0, part },
      { ...a, y, nx: 0, ny, nz: 0, part },
      { ...b, y, nx: 0, ny, nz: 0, part },
    ]);
  }
});
