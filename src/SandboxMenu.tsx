import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import type { KeyboardEvent, ReactNode } from "react";
import { deleteSandbox, openInT3, openSsh, pairAgain, pause, reinstallT3, resize, resume, setAutoSleep, sshCommand } from "./api";
import { BUSY, copyText, isAsleep, isFree } from "./common";
import type { Run } from "./common";
import { Check, ChevronDown, ChevronRight } from "./icons";
import type { AppState, SandboxView } from "./types";

interface Props {
  s: SandboxView;
  state: AppState;
  run: Run;
  /** The "…" button the menu hangs from. */
  anchor: HTMLElement;
  /** The popover box: the menu stays inside it. */
  container: HTMLElement;
  onConnect: (id: string) => void;
  onCopied: () => void;
  onClose: () => void;
}

const GAP = 4;
const EDGE = 8;

function Item({ children, onClick, disabled, danger }: { children: ReactNode; onClick: () => void; disabled?: boolean; danger?: boolean }) {
  return (
    <button role="menuitem" className={`menu-item${danger ? " danger" : ""}`} disabled={disabled} onClick={onClick}>
      {children}
    </button>
  );
}

const Sep = () => <div className="menu-sep" role="separator" />;

export default function SandboxMenu({ s, state, run, anchor, container, onConnect, onCopied, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const hint = useId();
  const [sub, setSub] = useState<"size" | "sleep" | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [pos, setPos] = useState<{ top: number; right: number } | null>(null);
  const focused = useRef(false);

  const close = (refocus: boolean) => {
    onClose();
    if (refocus) anchor.focus();
  };
  const act = (f: () => Promise<unknown>) => {
    close(true);
    void run(f);
  };
  const items = () => [...(ref.current?.querySelectorAll<HTMLButtonElement>('[role^="menuitem"]:not(:disabled)') ?? [])];

  // Under the button and right-aligned to it; above it when there is no room below.
  useLayoutEffect(() => {
    const m = ref.current;
    if (!m) return;
    const a = anchor.getBoundingClientRect();
    const c = container.getBoundingClientRect();
    const h = m.offsetHeight;
    const inner = container.clientHeight;
    let top = a.bottom - c.top - container.clientTop + GAP;
    if (top + h > inner - EDGE) {
      const up = a.top - c.top - container.clientTop - GAP - h;
      top = up >= EDGE ? up : Math.max(EDGE, inner - EDGE - h);
    }
    const right = Math.max(EDGE, c.right - a.right - container.clientLeft);
    setPos((p) => (p && p.top === top && p.right === right ? p : { top, right }));
  }, [anchor, container, sub, confirmDelete]);

  useEffect(() => {
    if (pos && !focused.current) {
      focused.current = true;
      items()[0]?.focus();
    }
  }, [pos]);

  useEffect(() => {
    const down = (e: Event) => {
      const t = e.target as Node;
      if (ref.current?.contains(t) || anchor.contains(t)) return;
      onClose();
    };
    const key = (e: globalThis.KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      onClose();
      anchor.focus();
    };
    const scroll = (e: Event) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    document.addEventListener("mousedown", down);
    document.addEventListener("keydown", key);
    document.addEventListener("scroll", scroll, true);
    return () => {
      document.removeEventListener("mousedown", down);
      document.removeEventListener("keydown", key);
      document.removeEventListener("scroll", scroll, true);
    };
  }, [anchor, onClose]);

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
    else if (e.key === "Tab") onClose();
  };

  const busy = BUSY.includes(s.state);
  const free = isFree(s, state.tier);
  const copySsh = () => {
    close(true);
    // The command is asked for only now: each new one may mint a 30-day token.
    void run(() => copyText(sshCommand(s.id))).then((ok) => ok && onCopied());
  };

  const subItem = (key: "size" | "sleep", label: string, value: string) => (
    <button
      role="menuitem"
      className="menu-item has-sub"
      aria-expanded={sub === key}
      disabled={free}
      onClick={() => setSub(sub === key ? null : key)}
    >
      <span>{label}</span>
      <span className="menu-value">
        {value}
        {sub === key ? <ChevronDown /> : <ChevronRight />}
      </span>
    </button>
  );

  return (
    <div
      ref={ref}
      role="menu"
      aria-label={`${s.name} actions`}
      className="menu"
      style={pos ? { top: pos.top, right: pos.right } : { top: 0, right: EDGE, visibility: "hidden" }}
      onKeyDown={onKeyDown}
    >
      {s.connected ? (
        <Item disabled={busy} onClick={() => act(() => openInT3(s.id))}>
          Open in T3 Code
        </Item>
      ) : (
        <Item
          disabled={busy}
          onClick={() => {
            close(true);
            onConnect(s.id);
          }}
        >
          Connect to T3 Code
        </Item>
      )}
      {s.connected && <Item onClick={() => act(() => pairAgain(s.id))}>Pair again</Item>}
      <Sep />
      {s.state === "running" && <Item onClick={() => act(() => pause(s.id))}>Pause</Item>}
      {isAsleep(s) && <Item onClick={() => act(() => resume(s.id))}>Resume</Item>}
      {subItem("size", "Resize", s.size)}
      {sub === "size" && (
        <div role="group" aria-label="Sizes" className="menu-sub">
          <p id={hint} className="menu-hint">
            Restarts the sandbox
          </p>
          {state.sizes.map((z) => {
            const on = z.label === s.size;
            return (
              <button
                key={z.label}
                role="menuitemradio"
                aria-checked={on}
                aria-describedby={hint}
                className="menu-item choice"
                onClick={() => (on ? close(true) : act(() => resize(s.id, z.vcpus, z.memMiB)))}
              >
                <span>{z.label}</span>
                {on && <Check />}
              </button>
            );
          })}
        </div>
      )}
      {subItem("sleep", "Auto sleep", s.autoSleep.label)}
      {sub === "sleep" && (
        <div role="group" aria-label="Auto sleep choices" className="menu-sub">
          {state.sleepChoices.map((c) => {
            const on = c.seconds === s.autoSleep.seconds;
            return (
              <button
                key={c.seconds}
                role="menuitemradio"
                aria-checked={on}
                className="menu-item choice"
                onClick={() => (on ? close(true) : act(() => setAutoSleep(s.id, c.seconds)))}
              >
                <span>{c.label}</span>
                {on && <Check />}
              </button>
            );
          })}
        </div>
      )}
      <Sep />
      <Item onClick={copySsh}>Copy SSH command</Item>
      <Item onClick={() => act(() => openSsh(s.id))}>Open in Terminal</Item>
      <Sep />
      {s.connected && !s.desktopBehind && <Item onClick={() => act(() => reinstallT3(s.id))}>Reinstall T3</Item>}
      <Item danger onClick={() => (confirmDelete ? act(() => deleteSandbox(s.id)) : setConfirmDelete(true))}>
        {confirmDelete ? "Confirm delete" : "Delete sandbox"}
      </Item>
    </div>
  );
}
