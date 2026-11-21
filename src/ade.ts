import type { SoonId } from "./ades";
import type { AdeId } from "./types";

/**
 * Every ADE the UI draws: the ones the app opens and the ones coming soon. Only looks are keyed
 * by it (logos, backgrounds, the hero). What the app does with an ADE is in ades.tsx.
 */
export type Ade = AdeId | SoonId;

export const ADES: { id: Ade; name: string }[] = [
  { id: "t3", name: "T3 Code" },
  { id: "cmux", name: "cmux" },
  { id: "orca", name: "Orca" },
  { id: "superset", name: "Superset" },
  { id: "paradev", name: "Paradev" },
];

export const adeName = (ade: Ade) => ADES.find((item) => item.id === ade)!.name;
