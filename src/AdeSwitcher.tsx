import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import type { KeyboardEvent } from "react";
import { AdeBadge, COMING_SOON, modeName } from "./ades";
import type { Mode } from "./ades";
import { thisComputer } from "./common";
import unfold from "./assets/main-view/unfold.svg";
import { Check } from "./icons";
import type { AdeView, AppState } from "./types";

interface Props {
  state: AppState;
  mode: Mode;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onChoose: (mode: Mode) => void;
}

const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? "" : "es"}`;

/** "0.1.10 on this Mac · on 3 sandboxes", or "Not on this Mac". */
function adeLine(a: AdeView, state: AppState) {
  const here = a.version ? `${a.version} on ${thisComputer()}` : `Not on ${thisComputer()}`;
  const n = state.sandboxes.filter((s) => s.ades.includes(a.id)).length;
  return n > 0 ? `${here} · on ${plural(n, "sandbox")}` : here;
}

/** The header button that shows the mode, and its menu of modes. */
export default function AdeSwitcher({ state, mode, open, onOpenChange, onChoose }: Props) {
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  // The menu goes on the popover itself, so no header clips it.
  const container = button.current?.closest<HTMLElement>(".popover");
  const name = modeName(state, mode);

  const close = (refocus: boolean) => {
    onOpenChange(false);
    if (refocus) button.current?.focus();
  };
  const items = () => [...(menu.current?.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]:not(:disabled)') ?? [])];

  useEffect(() => {
    if (!open) return;
    (items().find((b) => b.getAttribute("aria-checked") === "true") ?? items()[0])?.focus();
    const down = (e: Event) => {
      const t = e.target as Node;
      if (menu.current?.contains(t) || button.current?.contains(t)) return;
      onOpenChange(false);
    };
    const key = (e: globalThis.KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      onOpenChange(false);
      button.current?.focus();
    };
    document.addEventListener("mousedown", down);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("mousedown", down);
      document.removeEventListener("keydown", key);
    };
  }, [open, onOpenChange]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const list = items();
    if (list.length === 0) return;
    const i = list.indexOf(document.activeElement as HTMLButtonElement);
    const to = (n: number) => {
      e.preventDefault();
      list[(n + list.length) % list.length].focus();
    };
    if (e.key === "ArrowDown") to(i + 1);
    else if (e.key === "ArrowUp") to(i < 0 ? -1 : i - 1);
    else if (e.key === "Home") to(0);
    else if (e.key === "End") to(-1);
    else if (e.key === "Tab") onOpenChange(false);
  };

  const pick = (m: Mode) => {
    close(true);
    onChoose(m);
  };

  const item = (m: Mode, title: string, line: string) => (
    <button key={m} role="menuitemradio" aria-checked={m === mode} className="ade-item" onClick={() => pick(m)}>
      <AdeBadge id={m} size={24} />
      <span className="ade-item-text">
        <span className="strong">{title}</span>
        <span className="small muted">{line}</span>
      </span>
      {m === mode && <Check />}
    </button>
  );

  return (
    <>
      <button
        ref={button}
        className="ade-picker"
        title={name}
        aria-label={`ADE: ${name}. Switch ADE`}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => onOpenChange(!open)}
      >
        <AdeBadge id={mode} />
        <img src={unfold} alt="" />
      </button>
      {open && container && createPortal(
        <div ref={menu} role="menu" aria-label="Open sandboxes in" className="menu ade-menu" onKeyDown={onKeyDown}>
          {item("instabox", "InstaBox", "Sandboxes only, no ADE")}
          <div className="menu-sep" role="separator" />
          <span className="ade-menu-label">Open sandboxes in an ADE</span>
          {state.ades.map((a) => item(a.id, a.name, adeLine(a, state)))}
          {COMING_SOON.map((a) => (
            <button key={a.id} role="menuitemradio" aria-checked={false} disabled className="ade-item soon">
              <AdeBadge id={a.id} size={24} />
              <span className="ade-item-text">
                <span className="strong muted">{a.name}</span>
                <span className="small muted">Not supported yet</span>
              </span>
              <span className="tag">Coming soon</span>
            </button>
          ))}
          <div className="menu-sep" role="separator" />
          <p className="ade-menu-note">Switching changes the background and the actions on each sandbox. Every sandbox stays in the list.</p>
        </div>,
        container,
      )}
    </>
  );
}
