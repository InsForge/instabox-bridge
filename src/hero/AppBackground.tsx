import type { Ade } from "../ade";
import type { Mode } from "../ades";
import HeroAtmosphere from "./HeroAtmosphere";
import { useHeroSettings } from "./HeroSettings";

/** The look of a mode: plain Instabox shares T3's plain black background. */
export const themeOf = (mode: Mode): Ade => (mode === "instabox" ? "t3" : mode);

export default function AppBackground({ mode }: { mode: Mode }) {
  const ade = themeOf(mode);
  const settings = useHeroSettings(ade);

  return <div className="app-background" style={settings.style} aria-hidden="true">
    <div className="main-hero">
      <HeroAtmosphere ade={ade} patternSettings={settings.values} backgroundOnly={ade !== "paradev"} />
    </div>
  </div>;
}
