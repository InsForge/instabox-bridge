import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { HeroCube } from './HeroCube';
import { loadAdeLogo, type CubeLogo } from './ade-logo';

const renderer = vi.hoisted(() => ({ draw: vi.fn(), dispose: vi.fn(), setLogo: vi.fn() }));
vi.mock('./hero-cube-lighting', () => ({ createCubeLighting: () => renderer }));
vi.mock('./ade-logo', async (original) => ({
  ...await original<typeof import('./ade-logo')>(), loadAdeLogo: vi.fn(async () => null),
}));
let frames: Map<number, FrameRequestCallback>;
let nextFrame = 0;

beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers();
  frames = new Map();
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
    frames.set(++nextFrame, callback);
    return nextFrame;
  });
  vi.stubGlobal('cancelAnimationFrame', (id: number) => frames.delete(id));
  vi.stubGlobal('matchMedia', () => ({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
  vi.stubGlobal('IntersectionObserver', class { observe() {} disconnect() {} });
});

test('ADE changes replace the logo and preserve pose, with an immediately open lid under reduced motion', async () => {
  const view = render(<HeroCube ade="t3" />);
  advance();
  fireEvent.click(screen.getByRole('img', { name: /Interactive black metallic cube/ }));
  fireEvent.keyDown(screen.getByRole('img', { name: /Interactive black metallic cube/ }), { key: 'ArrowRight' });
  advance();
  const pose = document.querySelector<HTMLElement>('.hero-cube-cube')!.style.transform;
  const logo = { canvas: document.createElement('canvas'), pixels: new Uint8ClampedArray(4), width: 180, height: 180 };
  vi.mocked(loadAdeLogo).mockResolvedValueOnce(logo);
  view.rerender(<HeroCube ade="orca" />);
  await act(async () => {});
  expect(renderer.setLogo).toHaveBeenLastCalledWith(logo);
  expect(screen.getByTitle('Orca · Move your pointer to tilt')).toBeTruthy();
  expect(state().lift).toBe('20px');
  expect(document.querySelector<HTMLElement>('.hero-cube-cube')!.style.transform).toBe(pose);
  expect(renderer.dispose).not.toHaveBeenCalled();
});

test('a late logo load cannot overwrite the latest ADE selection', async () => {
  let finishOld!: (logo: CubeLogo | null) => void;
  vi.mocked(loadAdeLogo).mockImplementationOnce(() => new Promise((resolve) => { finishOld = resolve; }));
  const view = render(<HeroCube ade="t3" />);
  const current = { canvas: document.createElement('canvas'), pixels: new Uint8ClampedArray(4), width: 180, height: 180 };
  vi.mocked(loadAdeLogo).mockResolvedValueOnce(current);
  view.rerender(<HeroCube ade="cmux" />);
  await act(async () => {});
  await act(async () => { finishOld(null); });
  expect(renderer.setLogo).toHaveBeenLastCalledWith(current);
});

test('switching ADE replays the lid reveal without rebuilding the renderer or restarting on ordinary rerenders', () => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
  const view = render(<HeroCube ade="t3" />);
  for (let i = 0; i < 180; i++) advance(16);
  expect(state().lift).toBe('20px');

  view.rerender(<HeroCube ade="orca" />);
  expect(state().lift).toBe('0px');
  advance(16);
  const opening = parseFloat(state().lift);
  expect(opening).toBeGreaterThan(0);
  expect(opening).toBeLessThan(20);
  view.rerender(<HeroCube ade="orca" />);
  expect(parseFloat(state().lift)).toBe(opening);

  // A second switch while opening starts the new ADE's reveal cleanly.
  view.rerender(<HeroCube ade="cmux" />);
  expect(state().lift).toBe('0px');
  expect(frames.size).toBe(1);
  for (let i = 0; i < 180; i++) advance(16);
  expect(state().lift).toBe('20px');
  expect(renderer.dispose).not.toHaveBeenCalled();
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function advance(ms = 0) {
  act(() => {
    vi.advanceTimersByTime(ms);
    const pending = [...frames.values()];
    frames.clear();
    pending.forEach((callback) => callback(performance.now()));
  });
}

function state() {
  return {
    lift: document.querySelector<HTMLElement>('.hero-cube-cube')!.style.getPropertyValue('--lid-offset'),
    color: document.querySelector<HTMLElement>('.hero-cube-scene')!.style.getPropertyValue('--lid-color'),
  };
}

function pointer(type: string) {
  const event = new Event(type);
  Object.defineProperty(event, 'pointerType', { value: 'mouse' });
  fireEvent(screen.getByRole('img', { name: /Interactive black metallic cube/ }), event);
  advance();
}

test('first load opens the lid immediately and keeps it open', () => {
  render(<HeroCube />);
  advance();
  expect(state()).toEqual({ lift: '20px', color: '#FFFFFF' });
  advance(60000);
  expect(state().lift).toBe('20px');
});

test('the initial reveal animates open and clicks leave it open', () => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
  render(<HeroCube />);
  expect(state().lift).toBe('0px');
  advance();
  expect(parseFloat(state().lift)).toBeGreaterThan(0);
  expect(parseFloat(state().lift)).toBeLessThan(20);
  for (let i = 0; i < 30; i++) advance(16);
  expect(parseFloat(state().lift)).toBeLessThan(18);
  for (let i = 0; i < 120; i++) advance(16);
  expect(state().lift).toBe('20px');
  fireEvent.click(screen.getByRole('img', { name: /Interactive black metallic cube/ }));
  for (let i = 0; i < 60; i++) advance(16);
  expect(state().lift).toBe('20px');
});

test('click, hover and focus leave the lid open', () => {
  render(<HeroCube />);
  advance(1000);
  const cube = screen.getByRole('img', { name: /Interactive black metallic cube/ });
  fireEvent.click(cube);
  advance();
  expect(state().lift).toBe('20px');
  pointer('pointerenter');
  fireEvent.focus(cube);
  advance(10000);
  expect(state().lift).toBe('20px');
  fireEvent.click(cube);
  advance();
  pointer('pointerleave');
  fireEvent.blur(cube);
  advance(60000);
  expect(state().lift).toBe('20px');
});

test('clicking during the opening animation does not close it and unmount cancels frames', () => {
  const view = render(<HeroCube />);
  const cube = screen.getByRole('img', { name: /Interactive black metallic cube/ });
  fireEvent.click(cube);
  advance(1000);
  expect(state().lift).toBe('20px');
  view.unmount();
  const fresh = render(<HeroCube />);
  fresh.unmount();
  advance(2000);
  expect(frames.size).toBe(0);
});

 test('arrow keys tilt the cube and Escape resets only its pose', () => {
  render(<HeroCube />);
  advance();
  const control = screen.getByRole('img', { name: /Interactive black metallic cube/ });
  const cube = document.querySelector<HTMLElement>('.hero-cube-cube')!;
  const rest = cube.style.transform;
  fireEvent.keyDown(control, { key: 'ArrowRight' });
  advance();
  expect(cube.style.transform).not.toBe(rest);
  fireEvent.keyDown(control, { key: 'Escape' });
  advance();
  expect(cube.style.transform).toBe(rest);
  expect(state().lift).toBe('20px');
});

test('mouse travel tilts the cube and leaving settles back to rest', () => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
  render(<HeroCube />);
  const control = screen.getByRole('img', { name: /Interactive black metallic cube/ });
  const cube = document.querySelector<HTMLElement>('.hero-cube-cube')!;
  const send = (type: string, x: number, time: number) => {
    const event = new Event(type);
    Object.defineProperties(event, {
      pointerType: { value: 'mouse' }, buttons: { value: 0 },
      clientX: { value: x }, clientY: { value: 60 }, timeStamp: { value: time },
    });
    fireEvent(control, event);
  };
  advance();
  const rest = cube.style.transform;
  send('pointerenter', 10, 10);
  send('pointermove', 80, 40);
  advance(16);
  expect(cube.style.transform).not.toBe(rest);
  send('pointerleave', 80, 50);
  for (let i = 0; i < 240; i++) advance(16);
  expect(cube.style.transform).toBe(rest);
});
