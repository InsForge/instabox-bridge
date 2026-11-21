type Point = { x: number; y: number };
const corners = [-200, 200].flatMap((x) => [-200, 200].flatMap((y) => [-200, 200].map((z) => ({ x, y, z }))));
const cross = (a: Point, b: Point, c: Point) => (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);

/** Project the rotated case onto a floor beneath an overhead area light. */
export function stageProjection(rotation: { x: number; y: number; z: number }, height = 0, scale = 1) {
  const [x, y, z] = [rotation.x, rotation.y, rotation.z].map((angle) => (angle * Math.PI) / 180);
  const [sx, cx, sy, cy, sz, cz] = [Math.sin(x), Math.cos(x), Math.sin(y), Math.cos(y), Math.sin(z), Math.cos(z)];
  // CSS rotateX rotateY rotateZ applies Z, then Y, then X.
  const rotated = corners.map((p) => {
    const x1 = p.x * cz - p.y * sz,
      y1 = p.x * sz + p.y * cz;
    const x2 = x1 * cy + p.z * sy,
      z2 = -x1 * sy + p.z * cy;
    return { x: x2 * scale, y: (y1 * cx - z2 * sx) * scale, z: (y1 * sx + z2 * cx) * scale };
  });
  const lift = 200 - Math.max(...rotated.map((p) => p.y));
  const points = rotated
    .map((p) => {
      // Centered overhead light. Height lowers the receiving floor, not the case.
      const t = (1004 + height) / (p.y + lift + 800);
      const floorX = p.x * t;
      const floorZ = p.z * t;
      const perspective = 1400 / (1600 - floorZ);
      return { x: 200 + floorX * perspective, y: 200 + (204 + height) * perspective };
    })
    .sort((a, b) => a.x - b.x || a.y - b.y);
  const hull = (ordered: Point[]) => {
    const result: Point[] = [];
    for (const p of ordered) {
      while (result.length > 1 && cross(result[result.length - 2], result[result.length - 1], p) <= 0) result.pop();
      result.push(p);
    }
    return result.slice(0, -1);
  };
  const outline = [...hull(points), ...hull([...points].reverse())];
  // Align the projected footprint with the center of the illuminated floor patch.
  const centerX = (Math.min(...outline.map((p) => p.x)) + Math.max(...outline.map((p) => p.x))) / 2;
  const centerY = (Math.min(...outline.map((p) => p.y)) + Math.max(...outline.map((p) => p.y))) / 2;
  return {
    lift,
    shadow: outline
      .map((p) => `${(p.x - centerX + 200).toFixed(2)},${(p.y - centerY + 400 + height).toFixed(2)}`)
      .join(' '),
  };
}
