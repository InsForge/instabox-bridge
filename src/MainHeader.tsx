import { useEffect, useRef, useState } from "react";
import { HeroCube } from "./hero/HeroCube";
import symbol from "./assets/main-view/instabox-symbol.svg";
import wordmark from "./assets/main-view/instabox-wordmark.svg";
import { adeLogoUrl } from "./hero/ade-logo";
import unfold from "./assets/main-view/unfold.svg";
import { ADES } from "./ade";
import type { Ade } from "./ade";
import SelectMenu from "./SelectMenu";
import HeroAtmosphere from "./hero/HeroAtmosphere";
import { useHeroSettings } from "./hero/HeroSettings";

function AdeLogo({ ade }: { ade: Ade }) {
  return <span className="ade-logo" data-ade={ade} aria-hidden="true"><img src={adeLogoUrl(ade)} alt="" /></span>;
}

export default function MainHeader({ ade, onAdeChange }: { ade: Ade; onAdeChange: (ade: Ade) => void }) {
  const selected = ADES.find((item) => item.id === ade)!;
  const settings = useHeroSettings(ade);
  const toolbar = useRef<HTMLElement>(null);
  const [stuck, setStuck] = useState(false);

  useEffect(() => {
    const nav = toolbar.current;
    const scroll = nav?.closest(".scroll");
    const heading = scroll?.querySelector(".list-head");
    if (!nav || !scroll || !heading) return;
    // The solid header begins when the sandbox heading docks below the nav.
    const update = () => setStuck(heading.getBoundingClientRect().top <= nav.getBoundingClientRect().bottom);
    update();
    scroll.addEventListener("scroll", update, { passive: true });
    window.addEventListener("resize", update);
    return () => {
      scroll.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
    };
  }, []);

  return (
    <>
      <header ref={toolbar} className="main-hero-toolbar" data-stuck={stuck}>
        <div className="main-wordmark" role="img" aria-label="Instabox">
          <img className="main-wordmark-symbol" src={symbol} alt="" />
          <img className="main-wordmark-text" src={wordmark} alt="" />
        </div>
        <SelectMenu className="ade-picker" label="Development environment" value={ade}
          onChange={(value) => onAdeChange(value as Ade)}
          options={ADES.map((item) => ({ value: item.id, label: item.name,
            content: <><AdeLogo ade={item.id} /><span className="selection-label">{item.name}</span></> }))}>
          <AdeLogo ade={selected.id} />
          <img src={unfold} alt="" />
        </SelectMenu>
      </header>
      <div className="main-hero" style={settings.style}>
        <HeroAtmosphere ade={ade} patternSettings={settings.values} />
        <HeroCube ade={ade} />
      </div>
    </>
  );
}
