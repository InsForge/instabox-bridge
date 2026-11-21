import type { CSSProperties } from 'react';

export const heroDefaults = {
  cubeColor: '#0D0D0D',
  light: 60,
  position: 0,
  lightY: -360,
  lightZ: 940,
  texture: 0,
  lidLift: 20,
  lidLight: 100,
  lidGlow: 60,
  lidDuration: 500,
  floatHeight: 120,
  size: 86,
  x: -32,
  y: -45,
  z: 0,
};

// Use the landing page material and lighting configuration in the desktop header.
export const heroDefaultStyle = {
  '--case-light': String(heroDefaults.light / 400),
  '--case-light-offset': `${heroDefaults.position}%`,
  '--case-light-y': String(heroDefaults.lightY),
  '--case-light-z': String(heroDefaults.lightZ),
  '--case-grain': String(heroDefaults.texture * 0.003),
  '--lid-lift': String(heroDefaults.lidLift),
  '--lid-light': String(heroDefaults.lidLight / 100),
  '--lid-glow-spread': String(heroDefaults.lidGlow),
  '--lid-duration': String(heroDefaults.lidDuration),
  '--cube-color': heroDefaults.cubeColor,
} as CSSProperties;
