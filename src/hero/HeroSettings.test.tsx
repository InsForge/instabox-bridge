import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test } from 'vitest';
import type { Ade } from '../ade';
import HeroSettings, { useHeroSettings } from './HeroSettings';

function Preview({ ade }: { ade: Ade }) {
  const settings = useHeroSettings(ade);
  return <div className="popover"><header data-testid="hero" style={settings.style}><HeroSettings ade={ade} settings={settings} /></header></div>;
}
const adjust = (label: string, value: number) => fireEvent.change(screen.getByRole('slider', { name: label }), { target: { value: String(value) } });
const value = (name: string) => screen.getByTestId('hero').style.getPropertyValue(name);
beforeEach(() => localStorage.clear());
afterEach(() => { cleanup(); localStorage.clear(); });

test('opacity is live, independent per ADE, persists after remount, and resets only the current ADE', async () => {
  const view = render(<Preview ade="cmux" />);
  await userEvent.click(screen.getByRole('button', { name: 'Hero settings' }));
  adjust('Background opacity', 35);
  expect(value('--hero-background-opacity')).toBe('0.35');
  expect(value('--hero-cube-opacity')).toBe('1');
  view.rerender(<Preview ade="orca" />);
  expect(value('--hero-background-opacity')).toBe('1');
  adjust('Background opacity', 60);
  view.rerender(<Preview ade="cmux" />);
  expect(value('--hero-background-opacity')).toBe('0.35');
  view.unmount();
  const remount = render(<Preview ade="cmux" />);
  expect(value('--hero-background-opacity')).toBe('0.35');
  await userEvent.click(screen.getByRole('button', { name: 'Hero settings' }));
  await userEvent.click(screen.getByRole('button', { name: 'Reset' }));
  expect(value('--hero-background-opacity')).toBe('0.4');
  expect(value('--hero-fade-start')).toBe('0%');
  expect(value('--hero-fade-end')).toBe('100%');
  expect(value('--hero-cube-y')).toBe('64%');
  remount.rerender(<Preview ade="orca" />);
  expect(value('--hero-background-opacity')).toBe('0.6');
});

test('fade bounds stay ordered and Escape returns focus to the settings trigger', async () => {
  render(<Preview ade="paradev" />);
  const trigger = screen.getByRole('button', { name: 'Hero settings' });
  await userEvent.click(trigger);
  adjust('Fade starts', 95);
  expect(value('--hero-fade-end')).toBe('100%');
  adjust('Fully faded', 20);
  expect(value('--hero-fade-start')).toBe('15%');
  await userEvent.keyboard('{Escape}');
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(document.activeElement).toBe(trigger);
});

test('malformed stored values fall back or clamp to valid ranges', () => {
  localStorage.setItem('instabox.hero-preview.v1', JSON.stringify({ cmux: { backgroundOpacity: -20, cubeY: 1000, fadeStart: 90, fadeEnd: 10 } }));
  const view = render(<Preview ade="cmux" />);
  expect(value('--hero-background-opacity')).toBe('0');
  expect(value('--hero-cube-y')).toBe('80%');
  expect(value('--hero-fade-end')).toBe('95%');
  view.unmount();
  localStorage.setItem('instabox.hero-preview.v1', 'invalid json');
  render(<Preview ade="cmux" />);
  expect(value('--hero-background-opacity')).toBe('0.4');
});

test('Paradev applies the opacity preset once and preserves pattern tuning across ADEs and reloads', async () => {
  localStorage.setItem('instabox.hero-preview.v1', JSON.stringify({ paradev: { backgroundOpacity: 90, cubeOpacity: 70, fadeStart: 25 }, cmux: { backgroundOpacity: 35 } }));
  const view = render(<Preview ade="paradev" />);
  expect(value('--hero-background-opacity')).toBe('0.48');
  expect(value('--hero-cube-opacity')).toBe('1');
  expect(value('--hero-fade-start')).toBe('25%');
  await userEvent.click(screen.getByRole('button', { name: 'Hero settings' }));
  adjust('Pixel size', 4);
  adjust('Pattern horizontal position', -25);
  adjust('Pattern vertical position', 30);
  adjust('Pattern opacity', 65);
  adjust('Pattern density', 150);
  adjust('Animation speed', 0);
  adjust('Background opacity', 50);
  view.rerender(<Preview ade="cmux" />);
  expect(value('--hero-background-opacity')).toBe('0.35');
  expect(screen.queryByRole('slider', { name: 'Pixel size' })).toBeNull();
  view.unmount();
  render(<Preview ade="paradev" />);
  expect(value('--hero-background-opacity')).toBe('0.5');
  await userEvent.click(screen.getByRole('button', { name: 'Hero settings' }));
  for (const [name, expected] of [['Pattern horizontal position', '-25'], ['Pattern vertical position', '30'], ['Pixel size', '4'], ['Pattern opacity', '65'], ['Pattern density', '150'], ['Animation speed', '0']]) {
    expect((screen.getByRole('slider', { name }) as HTMLInputElement).value).toBe(expected);
  }
  await userEvent.click(screen.getByRole('button', { name: 'Reset' }));
  expect(value('--hero-background-opacity')).toBe('0.48');
  expect((screen.getByRole('slider', { name: 'Pixel size' }) as HTMLInputElement).value).toBe('2');
  expect(value('--hero-fade-start')).toBe('61%');
  expect(value('--hero-fade-end')).toBe('100%');
  expect((screen.getByRole('slider', { name: 'Pattern horizontal position' }) as HTMLInputElement).value).toBe('68');
  expect((screen.getByRole('slider', { name: 'Pattern vertical position' }) as HTMLInputElement).value).toBe('22');
});

test('Orca controls update the preview, persist independently, and reset with the current ADE', async () => {
  const view = render(<Preview ade="orca" />);
  await userEvent.click(screen.getByRole('button', { name: 'Hero settings' }));
  adjust('Orca speed', 200);
  adjust('Orca size', 150);
  adjust('Orca visibility', 30);
  adjust('Orca vertical position', 65);
  await userEvent.click(screen.getByRole('button', { name: 'Pause Orca animation' }));
  expect(value('--orca-duration')).toBe('14s');
  expect(value('--orca-distant-width')).toBe('174px');
  expect(value('--orca-width')).toBe('315px');
  expect(value('--orca-opacity')).toBe('0.3');
  expect(value('--orca-y')).toBe('65%');
  expect(value('--orca-play-state')).toBe('paused');
  view.rerender(<Preview ade="cmux" />);
  expect(screen.queryByRole('group', { name: 'Orca animation' })).toBeNull();
  adjust('Background opacity', 25);
  view.unmount();

  const remount = render(<Preview ade="orca" />);
  expect(value('--orca-duration')).toBe('14s');
  expect(value('--orca-opacity')).toBe('0.3');
  expect(value('--orca-play-state')).toBe('paused');
  await userEvent.click(screen.getByRole('button', { name: 'Hero settings' }));
  const pause = screen.getByRole('button', { name: 'Pause Orca animation' });
  expect(pause.getAttribute('aria-pressed')).toBe('true');
  await userEvent.click(pause);
  expect(value('--orca-play-state')).toBe('running');
  await userEvent.click(screen.getByRole('button', { name: 'Reset' }));
  expect(value('--orca-duration')).toBe('28s');
  expect(value('--orca-width')).toBe('273px');
  expect(value('--orca-opacity')).toBe('0.2');
  expect(value('--orca-y')).toBe('25%');
  remount.rerender(<Preview ade="cmux" />);
  expect(value('--hero-background-opacity')).toBe('0.25');
});

test('Orca settings safely load old preferences and reject invalid speed or pause values', () => {
  localStorage.setItem('instabox.hero-preview.v1', JSON.stringify({ orca: { backgroundOpacity: 70 } }));
  const view = render(<Preview ade="orca" />);
  expect(value('--hero-background-opacity')).toBe('0.7');
  expect(value('--orca-duration')).toBe('28s');
  expect(value('--orca-play-state')).toBe('running');
  view.unmount();
  localStorage.setItem('instabox.hero-preview.v1', JSON.stringify({ orca: { orcaSpeed: 0, orcaScale: 999, orcaOpacity: -20, orcaY: 'bad', orcaPaused: 'true' } }));
  render(<Preview ade="orca" />);
  expect(value('--orca-duration')).toBe('112s');
  expect(value('--orca-width')).toBe('630px');
  expect(value('--orca-opacity')).toBe('0');
  expect(value('--orca-y')).toBe('25%');
  expect(value('--orca-play-state')).toBe('running');
});
