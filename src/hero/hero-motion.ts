export type PointerSample = { x: number; y: number; time: number };
const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));

/** Follow travel direction, never the entry point's position on the cube. */
export function cubeHoverMotion(current: PointerSample, previous: PointerSample | null, previousSpeed: number) {
  const elapsed = previous ? current.time - previous.time : 0;
  const dx = previous ? current.x - previous.x : 0;
  const dy = previous ? current.y - previous.y : 0;
  const distance = Math.hypot(dx, dy);
  if (!previous || elapsed <= 0 || distance < 0.5) return null;
  const dt = clamp(elapsed / 1000, 0.008, 0.1);
  const measured = Math.min(distance / dt, 1800);
  const speed = previousSpeed + (measured - previousSpeed) * (1 - Math.exp(-dt / 0.04));
  const intensity = clamp(speed / 1400, 0, 1);
  const range = 3 + intensity * 7;
  return {
    speed,
    response: 10 + intensity * 12,
    rotation: { x: -(dy / distance) * range * 0.9, y: (dx / distance) * range, z: (dx / distance) * intensity },
  };
}
