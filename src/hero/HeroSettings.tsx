import { useEffect, useId, useRef, useState } from 'react';
import type { CSSProperties } from 'react';
import { createPortal } from 'react-dom';
import { ADES, adeName, type Ade } from '../ade';
import { normalizeParadevSettings, paradevPatternDefaults } from './paradev-pixels';
import './hero-settings.css';

const STORAGE_KEY = 'instabox.hero-preview.v1';
export const heroSettingsDefaults = {
  backgroundOpacity: 100, cubeOpacity: 100, fadeStart: 56, fadeEnd: 86, cubeY: 64,
  orcaSpeed: 100, orcaScale: 130, orcaOpacity: 20, orcaY: 25, orcaPaused: 0,
  paradevLightOpacity: 60, paradevLightWidth: 100, paradevLightReach: 200,
  ...paradevPatternDefaults,
};
type Settings = typeof heroSettingsDefaults;
type SettingsByAde = Record<Ade, Settings>;
const defaultsFor = (ade: Ade): Settings => ({
  ...heroSettingsDefaults,
  ...(ade === 'paradev' ? { backgroundOpacity: 48, cubeOpacity: 100, fadeStart: 61, fadeEnd: 100, pixelX: 68, pixelY: 22 } : {}),
  ...(ade === 'cmux' ? { backgroundOpacity: 40, fadeStart: 0, fadeEnd: 100 } : {}),
});
type Control = { key: keyof Settings; label: string; min: number; max: number; unit?: string };
const lightControls: Control[] = [
  { key: 'paradevLightOpacity', label: 'Light brightness', min: 0, max: 100 },
  { key: 'paradevLightWidth', label: 'Light width', min: 25, max: 200 },
  { key: 'paradevLightReach', label: 'Light downward reach', min: 25, max: 200 },
];
const patternControls: Control[] = [
  { key: 'pixelX', label: 'Pattern horizontal position', min: -100, max: 100 },
  { key: 'pixelY', label: 'Pattern vertical position', min: -100, max: 100 },
  { key: 'pixelOpacity', label: 'Pattern opacity', min: 0, max: 100 },
  { key: 'pixelSize', label: 'Pixel size', min: 1, max: 5, unit: 'px' },
  { key: 'pixelDensity', label: 'Pattern density', min: 0, max: 200 },
  { key: 'pixelSpeed', label: 'Animation speed', min: 0, max: 300 },
];
const orcaControls: Control[] = [
  { key: 'orcaSpeed', label: 'Orca speed', min: 25, max: 300 },
  { key: 'orcaScale', label: 'Orca size', min: 10, max: 300 },
  { key: 'orcaOpacity', label: 'Orca visibility', min: 0, max: 100 },
  { key: 'orcaY', label: 'Orca vertical position', min: 0, max: 100 },
];
const controls: Control[] = [
  { key: 'backgroundOpacity', label: 'Background opacity', min: 0, max: 100 },
  { key: 'cubeOpacity', label: 'Cube opacity', min: 0, max: 100 },
  { key: 'fadeStart', label: 'Fade starts', min: 0, max: 95 },
  { key: 'fadeEnd', label: 'Fully faded', min: 5, max: 100 },
  { key: 'cubeY', label: 'Cube vertical position', min: 40, max: 80 },
];

function readSettings(): SettingsByAde {
  let stored: Partial<Record<Ade, Partial<Settings>>> & { paradevOpacityVersion?: number } = {};
  if (import.meta.env.DEV) {
    try { stored = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}') ?? {}; } catch { /* Use defaults. */ }
  }
  return Object.fromEntries(ADES.map(({ id }) => {
    const values = defaultsFor(id);
    for (const { key, min, max } of [...controls, ...patternControls, ...lightControls, ...orcaControls]) {
      const value = stored[id]?.[key];
      if (typeof value === 'number' && Number.isFinite(value)) values[key] = Math.max(min, Math.min(max, value));
    }
    values.orcaPaused = stored[id]?.orcaPaused === 1 ? 1 : 0;
    // Apply the requested opacity preset once without discarding other tuning.
    if (id === 'paradev' && stored.paradevOpacityVersion !== 1) {
      values.backgroundOpacity = defaultsFor(id).backgroundOpacity;
      values.cubeOpacity = 100;
    }
    values.fadeEnd = Math.max(values.fadeStart + 5, values.fadeEnd);
    return [id, values];
  })) as SettingsByAde;
}

export function useHeroSettings(ade: Ade) {
  const [all, setAll] = useState(readSettings);
  const [saved, setSaved] = useState(true);
  useEffect(() => {
    if (!import.meta.env.DEV) return;
    try { localStorage.setItem(STORAGE_KEY, JSON.stringify({ ...all, paradevOpacityVersion: 1 })); setSaved(true); }
    catch { setSaved(false); }
  }, [all]);
  // Fast Refresh preserves older state objects: fill in newly introduced controls.
  const values = { ...defaultsFor(ade), ...all[ade], ...normalizeParadevSettings({ ...defaultsFor(ade), ...all[ade] }) };
  const change = (key: keyof Settings, value: number) => setAll((previous) => {
    const next = { ...defaultsFor(ade), ...previous[ade], ...normalizeParadevSettings({ ...defaultsFor(ade), ...previous[ade] }), [key]: value };
    if (key === 'fadeStart') next.fadeEnd = Math.max(next.fadeEnd, value + 5);
    if (key === 'fadeEnd') next.fadeStart = Math.min(next.fadeStart, value - 5);
    return { ...previous, [ade]: next };
  });
  const reset = () => setAll((previous) => ({ ...previous, [ade]: defaultsFor(ade) }));
  const style = {
    '--hero-background-opacity': values.backgroundOpacity / 100,
    '--hero-cube-opacity': values.cubeOpacity / 100,
    '--hero-fade-start': `${values.fadeStart}%`,
    '--hero-fade-end': `${values.fadeEnd}%`,
    '--hero-cube-y': `${values.cubeY}%`,
    '--paradev-light-opacity': values.paradevLightOpacity / 100,
    '--paradev-light-source-width': `${36 * values.paradevLightWidth / 100}%`,
    '--paradev-light-width': `${55 * values.paradevLightWidth / 100}%`,
    '--paradev-light-reach': `${values.paradevLightReach}%`,
    '--orca-duration': `${2800 / values.orcaSpeed}s`,
    '--orca-delay': `${-700 / values.orcaSpeed}s`,
    '--orca-swim-duration': `${560 / values.orcaSpeed}s`,
    '--orca-distant-duration': `${3900 / values.orcaSpeed}s`,
    '--orca-distant-delay': `${-2300 / values.orcaSpeed}s`,
    '--orca-distant-swim-duration': `${740 / values.orcaSpeed}s`,
    '--orca-distant-swim-delay': `${-300 / values.orcaSpeed}s`,
    '--orca-light-duration': `${1800 / values.orcaSpeed}s`,
    '--orca-water-light-duration': `${1400 / values.orcaSpeed}s`,
    '--orca-particle-duration': `${2400 / values.orcaSpeed}s`,
    '--orca-play-state': values.orcaPaused ? 'paused' : 'running',
    '--orca-width': `${210 * values.orcaScale / 100}px`,
    '--orca-distant-width': `${116 * values.orcaScale / 100}px`,
    '--orca-opacity': values.orcaOpacity / 100,
    '--orca-y': `${values.orcaY}%`,
  } as CSSProperties;
  return { values, style, change, reset, saved };
}

export default function HeroSettings({ ade, settings }: { ade: Ade; settings: ReturnType<typeof useHeroSettings> }) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const id = useId();
  const container = trigger.current?.closest<HTMLElement>('.popover');
  const close = () => { setOpen(false); trigger.current?.focus({ preventScroll: true }); };
  useEffect(() => { if (open) panel.current?.querySelector<HTMLInputElement>('input')?.focus({ preventScroll: true }); }, [open]);

  const renderControl = ({ key, label, min, max, unit = '%' }: Control) => <label className="hero-setting" key={key} htmlFor={`${id}-${key}`}>
    <span>{label}<output>{settings.values[key]}{unit}</output></span>
    <input id={`${id}-${key}`} aria-label={label} type="range" min={min} max={max} step="1" value={settings.values[key]}
      onChange={(event) => settings.change(key, Number(event.target.value))} />
  </label>;

  return <>
    <button ref={trigger} className="hero-tune-trigger" type="button" aria-label="Hero settings"
      aria-expanded={open} aria-controls={open ? id : undefined} onClick={() => setOpen(!open)}>Tune hero</button>
    {open && container && createPortal(<div ref={panel} id={id} className="hero-settings" role="dialog" aria-label="Hero settings"
      onKeyDown={(event) => { if (event.key === 'Escape') { event.stopPropagation(); close(); } }}>
      <div className="hero-settings-heading">
        <div><strong>{adeName(ade)} hero</strong><span>Temporary preview controls</span></div>
        <button className="icon-btn" aria-label="Close hero settings" onClick={close}>×</button>
      </div>
      <p className="hero-settings-hint">Lower background opacity to soften the color and decorations.</p>
      {controls.map(renderControl)}
      {ade === 'orca' && <fieldset className="hero-pattern-settings">
        <legend>Orca animation</legend>
        <button className="btn hero-orca-pause" type="button" aria-pressed={settings.values.orcaPaused === 1}
          aria-label="Pause Orca animation"
          onClick={() => settings.change('orcaPaused', settings.values.orcaPaused ? 0 : 1)}>
          {settings.values.orcaPaused ? 'Resume animation' : 'Pause animation'}
        </button>
        <p className="hero-settings-hint">100% is the original size and speed. Lower visibility for a subtler silhouette. Reduced motion keeps the scene still.</p>
        {orcaControls.map(renderControl)}
      </fieldset>}
      {ade === 'paradev' && <fieldset className="hero-pattern-settings">
        <legend>Top light</legend>
        {lightControls.map(renderControl)}
      </fieldset>}
      {ade === 'paradev' && <fieldset className="hero-pattern-settings">
        <legend>Pixelated pattern</legend>
        <p className="hero-settings-hint">Position: 0% is the original placement. Negative moves left or up; positive moves right or down.</p>
        {patternControls.map(renderControl)}
      </fieldset>}
      <div className="hero-settings-footer"><span>{settings.saved ? 'Saved locally for this ADE' : 'Changes last for this session'}</span>
        <button className="btn" onClick={settings.reset}>Reset</button></div>
    </div>, container)}
  </>;
}
