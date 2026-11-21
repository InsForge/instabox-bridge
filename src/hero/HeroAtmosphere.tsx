import { useEffect, useRef } from 'react';
import type { Ade } from '../ade';
import ParadevPixels from './ParadevPixels';
import type { ParadevPatternSettings } from './paradev-pixels';
import orcaUnderwater from '../assets/hero/orca-underwater.png';
import './hero-atmosphere.css';

export default function HeroAtmosphere({ ade, patternSettings }: { ade: Ade; patternSettings?: ParadevPatternSettings }) {
  const atmosphere = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (ade !== 'orca' && ade !== 'cmux') return;
    const visibility = () => {
      if (atmosphere.current) atmosphere.current.dataset.paused = String(document.hidden);
    };
    visibility();
    document.addEventListener('visibilitychange', visibility);
    return () => document.removeEventListener('visibilitychange', visibility);
  }, [ade]);

  return <>
    <div className="hero-atmosphere" ref={atmosphere} data-theme={ade} aria-hidden="true">
      {ade === 'orca' && <>
        <div className="orca-light-rays" />
        <div className="hero-orca-layer">
          {['distant', 'near'].map((depth) => <div className={`orca-current orca-current-${depth}`} key={depth}>
            <div className="orca-swimmer">
              <img className="swimming-orca" src={orcaUnderwater} alt="" width="2172" height="724" decoding="async" />
            </div>
          </div>)}
        </div>
        <div className="orca-water-light" />
        <div className="orca-marine-snow" />
      </>}
      {ade === 'paradev' && <ParadevPixels settings={patternSettings} />}
    </div>
  </>;
}
