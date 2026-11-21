import { adeLogoUrl } from "./hero/ade-logo";
import { MarkTile } from "./icons";
import type { AdeId, AppState, SandboxView } from "./types";

/** What the popover is for: plain Instabox (sandboxes only) or opening sandboxes in an ADE. */
export type Mode = "instabox" | AdeId;

/** An ADE the switcher lists but the app does not support yet. */
export type SoonId = "orca" | "superset" | "paradev";

/** The ADEs the switcher shows as coming soon, after the ones the app supports. */
export const COMING_SOON: { id: SoonId; name: string }[] = [
  { id: "orca", name: "Orca" },
  { id: "superset", name: "Superset" },
  { id: "paradev", name: "Paradev" },
];

/** Letters for an ADE with no logo image yet. */
const LETTERS: Partial<Record<AdeId | SoonId, string>> = { superset: "S" };

/** The mark of an ADE (its logo on a tile), or the instabox mark for plain Instabox. */
export function AdeBadge({ id, size = 20 }: { id: Mode | SoonId; size?: number }) {
  if (id === "instabox") return <MarkTile size={size} />;
  const logo = adeLogoUrl(id);
  return (
    <span className="ade-logo" data-ade={id} aria-hidden="true" style={{ width: size, height: size }}>
      {logo ? <img src={logo} alt="" /> : LETTERS[id]}
    </span>
  );
}

const NAMES: Record<AdeId, string> = { t3: "T3 Code", cmux: "cmux" };

/** The desktop app's name, such as "T3 Code". */
export const adeName = (state: AppState, id: AdeId) => state.ades.find((a) => a.id === id)?.name ?? NAMES[id];

/** The ADE's desktop app on this computer, if it runs here. */
export const localAde = (state: AppState, id: AdeId) => state.ades.find((a) => a.id === id);

/** The name of a mode, for the switcher. */
export const modeName = (state: AppState, mode: Mode) => (mode === "instabox" ? "InstaBox" : adeName(state, mode));

/** True when the ADE is set up on the sandbox (its mark lists it). */
export const hasAde = (s: SandboxView, id: AdeId) => s.ades.includes(id);

/** T3 pairs with a server it installs, and keeps its version in step; cmux opens a link. */
export const pairs = (id: AdeId) => id === "t3";

/** The line under a row that has the ADE. */
export function readyText(state: AppState, s: SandboxView, id: AdeId): string {
  if (id === "t3") return s.t3Version ? `T3 Code ${s.t3Version}` : "T3 Code";
  return `Ready for ${adeName(state, id)}`;
}
