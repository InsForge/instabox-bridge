import type { Ade } from '../ade';
import { adeLogoUrl } from './ade-logo';
// Opaque geometry fallback for browsers with no canvas support.
const cornerRadius = 28;
const cornerSegments = 24;
const roundedEdges = [
  [172, 172, 0],
  [-172, 172, 90],
  [-172, -172, 180],
  [172, -172, 270],
].flatMap(([cx, cz, start]) =>
  Array.from({ length: cornerSegments }, (_, index) => {
    const angle = start + ((index + 0.5) * 90) / cornerSegments;
    const radians = (angle * Math.PI) / 180;
    const halfStep = Math.PI / (4 * cornerSegments);
    const radius = cornerRadius * Math.cos(halfStep);
    const width = 2 * cornerRadius * Math.sin(halfStep) + 0.6;
    return {
      width,
      transform: `translate3d(${200 + cx + radius * Math.cos(radians)}px, 200px, ${cz + radius * Math.sin(radians)}px) rotateY(${90 - angle}deg) translate(-50%, -50%)`,
    };
  }),
);

/** Shared rounded metal shell for the hero and the agent cubes. */
export function CubeFaces({ ade }: { ade: Ade }) {
  return (
    <>
      {['front', 'back', 'left', 'right', 'top', 'bottom'].map((face) => (
        <span key={face} className={`hero-cube-face hero-cube-${face}`}>
          {face === 'top' && adeLogoUrl(ade) && <img className="hero-cube-top-logo" src={adeLogoUrl(ade)} alt="" />}
        </span>
      ))}
      {roundedEdges.map((style, index) => (
        <span key={index} className="hero-cube-rounded-edge" style={style} />
      ))}
    </>
  );
}
