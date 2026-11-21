// Adapted from the InstaBox landing page; shares its mesh, lighting and motion.
import { useEffect, useRef } from 'react';
import { cubeHoverMotion, type PointerSample } from './hero-motion';
import { stageProjection } from './hero-stage';
import { createCubeLighting } from './hero-cube-lighting';
import { heroDefaultStyle, heroDefaults as defaults } from './hero-config';
import { CubeFaces } from './CubeFaces';
import './hero-cube.css';
import { adeName, type Ade } from '../ade';
import { loadAdeLogo, type CubeLogo } from './ade-logo';

const initialCubeStyle = {
  transform: `translateY(${stageProjection(defaults, defaults.floatHeight, defaults.size / 100).lift}px) translateZ(-200px) rotateX(${defaults.x}deg) rotateY(${defaults.y}deg) rotateZ(${defaults.z}deg) scale3d(${defaults.size / 100}, ${defaults.size / 100}, ${defaults.size / 100})`,
};
const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));

export function HeroCube({ ade = 't3' }: { ade?: Ade }) {
  const logoController = useRef<((logo: CubeLogo | null) => void) | null>(null);
  const replayLid = useRef<(() => void) | null>(null);
  const sceneRef = useRef<HTMLDivElement>(null);
  const controlRef = useRef<HTMLDivElement>(null);
  const cubeRef = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    const control = controlRef.current;
    const cube = cubeRef.current;
    if (!control || !cube) return;

    const scene = sceneRef.current!;
    const lighting = createCubeLighting(cube.parentElement!, scene);
    const rest = { x: defaults.x, y: defaults.y, z: defaults.z };
    const motion = window.matchMedia('(prefers-reduced-motion: reduce)');
    const rotation = { ...rest };
    const target = { ...rest };
    let pointer: PointerSample | null = null;
    let pointerSpeed = 0;
    let response = 8;
    let returnAt = 0;
    let visible = true;
    let frame = 0;
    let last = 0;
    let activeUntil = 0;
    let stagePose = '';
    let lidProgress = 0;
    const lidTarget = 1;
    scene.style.setProperty('--lid-color', '#FFFFFF');
    const lidLift = defaults.lidLift;
    const floatHeight = defaults.floatHeight;
    const size = defaults.size;

    const paint = () => {
      const nextPose = `${rotation.x.toFixed(3)},${rotation.y.toFixed(3)},${rotation.z.toFixed(3)},${floatHeight},${size}`;
      if (stagePose !== nextPose) {
        stagePose = nextPose;
        const stage = stageProjection(rotation, floatHeight, size / 100);
        sceneRef.current?.style.setProperty('--stage-floor-offset', `${floatHeight}px`);
        cube.style.transform = `translateY(${stage.lift}px) translateZ(-200px) rotateX(${rotation.x}deg) rotateY(${rotation.y}deg) rotateZ(${rotation.z}deg) scale3d(${size / 100}, ${size / 100}, ${size / 100})`;
      }
      cube.style.setProperty('--lid-offset', `${lidProgress * lidLift}px`);
      lighting?.draw(
        rotation,
        size / 100,
        stageProjection(rotation, floatHeight, size / 100).lift,
        lidProgress * lidLift,
      );
    };
    const tick = (now: number) => {
      frame = 0;
      const dt = last ? Math.min((now - last) / 1000, 0.05) : 1 / 60;
      last = now;
      if (returnAt && now >= returnAt) {
        returnAt = 0;
        response = 8;
        Object.assign(target, rest);
      }
      const ease = motion.matches ? 1 : 1 - Math.exp(-response * dt);
      rotation.x += (target.x - rotation.x) * ease;
      rotation.y += (target.y - rotation.y) * ease;
      rotation.z += (target.z - rotation.z) * ease;
      for (const axis of ['x', 'y', 'z'] as const) {
        if (Math.abs(target[axis] - rotation[axis]) < 0.001) rotation[axis] = target[axis];
      }
      const duration = 1500;
      const lidEase = motion.matches ? 1 : 1 - Math.exp((-4.6 * dt) / (duration / 1000));
      lidProgress += (lidTarget - lidProgress) * lidEase;
      if (Math.abs(lidTarget - lidProgress) < 0.001) {
        lidProgress = lidTarget;
      }
      paint();
      const settled = !returnAt && lidProgress === lidTarget &&
        rotation.x === target.x && rotation.y === target.y && rotation.z === target.z;
      if (!settled && visible && !document.hidden && now < activeUntil) frame = requestAnimationFrame(tick);
      else last = 0;
    };
    const wake = () => {
      activeUntil = performance.now() + (motion.matches ? 0 : 6000);
      if (!frame && visible && !document.hidden) frame = requestAnimationFrame(tick);
    };
    logoController.current = (logo) => {
      lighting?.setLogo(logo);
      paint();
      wake();
    };
    replayLid.current = () => {
      lidProgress = motion.matches ? lidTarget : 0;
      last = 0;
      paint();
      wake();
    };
    const enter = (event: PointerEvent) => {
      if (event.pointerType === 'touch') return;
      if (motion.matches) return;
      pointer = { x: event.clientX, y: event.clientY, time: event.timeStamp };
      pointerSpeed = 0;
      returnAt = 0;
    };
    const move = (event: PointerEvent) => {
      if (event.pointerType === 'touch' || event.buttons !== 0 || motion.matches) return;
      const current = { x: event.clientX, y: event.clientY, time: event.timeStamp };
      const hover = cubeHoverMotion(current, pointer, pointerSpeed);
      pointer = current;
      if (!hover) return;
      pointerSpeed = hover.speed;
      response = hover.response;
      returnAt = 0;
      target.x = rest.x + hover.rotation.x;
      target.y = rest.y + hover.rotation.y;
      target.z = rest.z + hover.rotation.z;
      wake();
    };
    const leave = (event: PointerEvent) => {
      // Include the final movement so fast crossings turn toward the exit side.
      move(event);
      pointer = null;
      pointerSpeed = 0;
      // Let even a fast pass impart a short turn before easing back to the audience.
      returnAt = performance.now() + 120;
      wake();
    };
    const keydown = (event: KeyboardEvent) => {
      if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Escape', 'Home'].includes(event.key)) return;
      event.preventDefault();
      returnAt = 0;
      response = 12;
      if (event.key === 'ArrowLeft') target.y = clamp(target.y - 8, rest.y - 42, rest.y + 38);
      if (event.key === 'ArrowRight') target.y = clamp(target.y + 8, rest.y - 42, rest.y + 38);
      if (event.key === 'ArrowUp') target.x = clamp(target.x - 6, rest.x - 28, rest.x + 18);
      if (event.key === 'ArrowDown') target.x = clamp(target.x + 6, rest.x - 28, rest.x + 18);
      if (event.key === 'Escape' || event.key === 'Home') {
        Object.assign(target, rest);
      }
      target.z = rest.z + clamp((target.y - rest.y) * 0.2, -10, 10);
      wake();
    };
    const reset = () => {
      pointer = null;
      pointerSpeed = 0;
      returnAt = 0;
      response = 8;
      Object.assign(target, rest);
      wake();
    };
    const suspend = () => {
      cancelAnimationFrame(frame);
      frame = 0;
      last = 0;
    };
    const visibility = () => {
      if (document.hidden) {
        reset();
        suspend();
      } else wake();
    };
    const motionChange = () => {
      suspend();
      if (motion.matches) {
        reset();
        Object.assign(rotation, rest);
      }
      paint();
      wake();
    };
    const observer = new IntersectionObserver(([entry]) => {
      visible = entry.isIntersecting;
      if (visible) wake();
      else {
        reset();
        suspend();
      }
    });
    observer.observe(control);
    control.addEventListener('pointerenter', enter);
    control.addEventListener('pointermove', move);
    control.addEventListener('pointerleave', leave);
    control.addEventListener('pointercancel', reset);
    control.addEventListener('keydown', keydown);
    control.addEventListener('blur', reset);
    document.addEventListener('visibilitychange', visibility);
    motion.addEventListener('change', motionChange);
    paint();
    wake();
    return () => {
      logoController.current = null;
      replayLid.current = null;
      suspend();
      observer.disconnect();
      lighting?.dispose();
      control.removeEventListener('pointerenter', enter);
      control.removeEventListener('pointermove', move);
      control.removeEventListener('pointerleave', leave);
      control.removeEventListener('pointercancel', reset);
      control.removeEventListener('keydown', keydown);
      control.removeEventListener('blur', reset);
      document.removeEventListener('visibilitychange', visibility);
      motion.removeEventListener('change', motionChange);
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    replayLid.current?.();
    logoController.current?.(null);
    void loadAdeLogo(ade).then((logo) => {
      if (!cancelled) logoController.current?.(logo);
    });
    return () => { cancelled = true; };
  }, [ade]);

  return (
    <div className="hero-cube-scene" ref={sceneRef} style={heroDefaultStyle} data-ade={ade}>
      <div
        ref={controlRef}
        role="img"
        tabIndex={0}
        className="hero-cube-control"
        title={`${adeName(ade)} · Move your pointer to tilt`}
        aria-label="Interactive black metallic cube. Move your mouse across it or use arrow keys to tilt. Escape resets the rotation."
      >
        <span className="hero-cube-scale" aria-hidden="true">
          <span className="hero-cube-perspective">
            <span className="hero-cube-cube" ref={cubeRef} style={initialCubeStyle}>
              <CubeFaces ade={ade} />
            </span>
          </span>
        </span>
      </div>
    </div>
  );
}
