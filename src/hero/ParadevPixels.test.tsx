import { cleanup, render } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import ParadevPixels from './ParadevPixels';
import { drawParadevPixels } from './paradev-pixels';

const ctx = { clearRect: vi.fn(), setTransform: vi.fn(), fillRect: vi.fn(), fillStyle: '' };
let callbacks: Map<number, FrameRequestCallback>;
let motion: { matches: boolean; addEventListener: ReturnType<typeof vi.fn>; removeEventListener: ReturnType<typeof vi.fn> };
let hidden: boolean;
const disconnect = vi.fn();

beforeEach(() => {
  callbacks = new Map();
  let id = 0;
  hidden = false;
  motion = { matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() };
  vi.spyOn(document, 'hidden', 'get').mockImplementation(() => hidden);
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(ctx as unknown as CanvasRenderingContext2D);
  vi.spyOn(HTMLCanvasElement.prototype, 'clientWidth', 'get').mockReturnValue(360);
  vi.spyOn(HTMLCanvasElement.prototype, 'clientHeight', 'get').mockReturnValue(201);
  vi.stubGlobal('matchMedia', () => motion);
  vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => { callbacks.set(++id, cb); return id; });
  vi.stubGlobal('cancelAnimationFrame', (key: number) => callbacks.delete(key));
  class Observer { observe() {} disconnect = disconnect; }
  vi.stubGlobal('ResizeObserver', Observer);
  vi.stubGlobal('IntersectionObserver', Observer);
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.clearAllMocks(); });

test('pixels evolve over time while staying on the square grid', () => {
  drawParadevPixels(ctx as unknown as CanvasRenderingContext2D, 360, 201, 0);
  const initial = ctx.fillRect.mock.calls.map(args => [...args]);
  ctx.fillRect.mockClear();
  drawParadevPixels(ctx as unknown as CanvasRenderingContext2D, 360, 201, 150);
  expect(ctx.fillRect.mock.calls).not.toEqual(initial);
  expect(initial.length).toBeGreaterThan(100);
  for (const [x, y, w, h] of ctx.fillRect.mock.calls) {
    expect(x % 4).toBe(0);
    expect(y % 4).toBe(0);
    expect([w, h]).toEqual([2, 2]);
  }
});

test('pauses when hidden and cancels animation and observers on unmount', () => {
  const view = render(<ParadevPixels />);
  expect(callbacks.size).toBe(1);
  hidden = true;
  document.dispatchEvent(new Event('visibilitychange'));
  expect(callbacks.size).toBe(0);
  hidden = false;
  document.dispatchEvent(new Event('visibilitychange'));
  expect(callbacks.size).toBe(1);
  view.unmount();
  expect(callbacks.size).toBe(0);
  expect(disconnect).toHaveBeenCalledTimes(2);
  expect(motion.removeEventListener).toHaveBeenCalledWith('change', expect.any(Function));
});

test('reduced motion renders a static pattern without scheduling animation', () => {
  motion.matches = true;
  render(<ParadevPixels />);
  expect(ctx.fillRect).toHaveBeenCalled();
  expect(callbacks.size).toBe(0);
});

test('older live-preview settings keep sparse clusters moving after new controls are added', () => {
  // Fast Refresh retains the settings object from before pattern controls existed.
  const oldSettings = { pixelSize: 2 } as Parameters<typeof ParadevPixels>[0]['settings'];
  render(<ParadevPixels settings={oldSettings} />);
  const advance = (time: number) => {
    const pending = [...callbacks.values()];
    callbacks.clear();
    ctx.fillRect.mockClear();
    pending.forEach(callback => callback(time));
    return ctx.fillRect.mock.calls.map(args => [...args]);
  };
  advance(0);
  const first = advance(100);
  const later = advance(5100);
  expect(first.length).toBeGreaterThan(100);
  expect(first.length).toBeLessThan(360 / 4 * Math.ceil(201 / 4) * 0.4);
  expect(later).not.toEqual(first);
  expect(ctx.fillStyle).not.toContain('NaN');
});
