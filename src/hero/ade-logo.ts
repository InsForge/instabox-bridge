import type { Ade } from '../ade';
import t3Logo from '../assets/main-view/t3-logo.svg';
import orcaLogo from '../assets/ade/orca.svg';
import cmuxLogo from '../assets/ade/cmux.png';
import paradevLogo from '../assets/ade/paradev.svg';

/** Superset has no logo yet: it is drawn with its letter (AdeBadge). */
const logoUrls: Partial<Record<Ade, string>> = { t3: t3Logo, orca: orcaLogo, cmux: cmuxLogo, paradev: paradevLogo };
export const adeLogoUrl = (ade: Ade): string | undefined => logoUrls[ade];

export type CubeLogo = { canvas: HTMLCanvasElement; pixels: Uint8ClampedArray; width: number; height: number };
const logos = new Map<Ade, Promise<CubeLogo | null>>();

export function loadAdeLogo(ade: Ade): Promise<CubeLogo | null> {
  const cached = logos.get(ade);
  if (cached) return cached;
  const url = adeLogoUrl(ade);
  const result = new Promise<CubeLogo | null>((resolve) => {
    if (!url) { resolve(null); return; }
    const image = new Image();
    image.onload = () => {
      const canvas = document.createElement('canvas');
      canvas.width = 256;
      canvas.height = Math.round(256 * image.naturalHeight / image.naturalWidth);
      const context = canvas.getContext('2d');
      if (!context) { resolve(null); return; }
      context.drawImage(image, 0, 0, canvas.width, canvas.height);
      // T3's source mark is black; use its white variant on the dark metal lid.
      if (ade === 't3') {
        context.globalCompositeOperation = 'source-in';
        context.fillStyle = '#ffffff';
        context.fillRect(0, 0, canvas.width, canvas.height);
      }
      resolve({ canvas, pixels: context.getImageData(0, 0, canvas.width, canvas.height).data,
        width: 180, height: 180 * canvas.height / canvas.width });
    };
    image.onerror = () => resolve(null);
    image.src = url;
  });
  logos.set(ade, result);
  return result;
}
