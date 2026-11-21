import { useEffect, useRef } from 'react';
import { drawParadevPixels, normalizeParadevSettings, paradevPatternDefaults, type ParadevPatternSettings } from './paradev-pixels';

const FRAME_MS = 1000 / 15;

export default function ParadevPixels({ settings = paradevPatternDefaults }: { settings?: Partial<ParadevPatternSettings> }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const normalized = normalizeParadevSettings(settings);
  const options = useRef(normalized);
  const redraw = useRef<(() => void) | null>(null);
  useEffect(() => {
    options.current = normalizeParadevSettings(settings);
    redraw.current?.();
  }, [settings]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext('2d');
    if (!canvas || !ctx) return;

    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
    let width = 0;
    let height = 0;
    let frame = 0;
    let raf = 0;
    let last: number | null = null;
    let visible = true;
    const draw = () => drawParadevPixels(ctx, width, height, frame, options.current);
    redraw.current = draw;
    const resize = () => {
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      width = canvas.clientWidth;
      height = canvas.clientHeight;
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      draw();
    };
    const tick = (now: number) => {
      if (last === null) last = now;
      const elapsed = now - last;
      if (elapsed >= FRAME_MS) {
        // Time-based drift stays consistent on 60/120 Hz displays.
        frame += Math.min(elapsed, 200) / FRAME_MS * options.current.pixelSpeed / 100;
        last = now;
        draw();
      }
      raf = requestAnimationFrame(tick);
    };
    const syncMotion = () => {
      cancelAnimationFrame(raf);
      last = null;
      if (visible && !document.hidden && !reducedMotion.matches) {
        raf = requestAnimationFrame(tick);
      }
    };

    const resizeObserver = new ResizeObserver(resize);
    resizeObserver.observe(canvas);
    const intersectionObserver = new IntersectionObserver(([entry]) => {
      visible = entry?.isIntersecting ?? true;
      syncMotion();
    });
    intersectionObserver.observe(canvas);
    document.addEventListener('visibilitychange', syncMotion);
    reducedMotion.addEventListener('change', syncMotion);
    resize();
    syncMotion();

    return () => {
      redraw.current = null;
      cancelAnimationFrame(raf);
      resizeObserver.disconnect();
      intersectionObserver.disconnect();
      document.removeEventListener('visibilitychange', syncMotion);
      reducedMotion.removeEventListener('change', syncMotion);
    };
  }, []);

  return <canvas className="paradev-pixels" ref={canvasRef} style={{ opacity: normalized.pixelOpacity / 100 }} aria-hidden="true" />;
}
