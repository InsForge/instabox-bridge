import { useEffect, useId, useRef, useState } from "react";
import { closeT3Switch, copyT3SwitchUrl, toErrorView } from "./api";
import type { Run } from "./common";
import { Check, Close, Copy } from "./icons";
import type { T3SwitchView } from "./types";

/**
 * Moves a sandbox T3 Code saved with its published port to the local T3 proxy, in T3 Code's own
 * settings: add the proxy as a route of the saved environment, then remove the old route. The
 * environment stays, with its chats. Labels are T3 Code 0.1.13's.
 *
 * Done and Don't show again end the guide for the sandbox. Close (or Escape) shows it again
 * at the next Open in T3 Code.
 */
export default function T3SwitchGuide({ guide, run }: { guide: T3SwitchView; run: Run }) {
  const titleId = useId();
  const box = useRef<HTMLDivElement>(null);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const close = (done: boolean) => void run(() => closeT3Switch(guide.id, done));

  useEffect(() => {
    box.current?.focus();
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      void run(() => closeT3Switch(guide.id, false));
    };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [guide.id, run]);

  // Each copy is a new one-time link. The secret never reaches the UI.
  const copy = () => {
    setError(null);
    copyT3SwitchUrl(guide.id).then(
      () => setCopied(true),
      (e) => {
        setCopied(false);
        setError(toErrorView(e).message);
      },
    );
  };

  return (
    <div className="dialog-scrim">
      <div ref={box} className="dialog" role="dialog" aria-modal="true" aria-labelledby={titleId} tabIndex={-1}>
        <div className="dialog-head">
          <h2 id={titleId} className="dialog-title">
            Switch {guide.name} to the local connection
          </h2>
          <button className="icon-btn" aria-label="Close" onClick={() => close(false)}>
            <Close />
          </button>
        </div>
        <p className="muted">T3 Code still uses this sandbox's old address, which keeps it awake.</p>
        <ol className="dialog-steps">
          <li>In T3 Code, open Settings → Environments → Routes on this sandbox.</li>
          <li>
            Click Add route, paste this link, then click Add route.
            <div className="ssh dialog-copy">
              <code className="ssh-box" aria-label={`Pairing link for ${guide.newAddress}`}>
                <span>{guide.newAddress}/pair#token=••••••</span>
              </code>
              <button className="btn md" onClick={copy}>
                {copied ? <Check /> : <Copy />}
                {copied ? "Copied" : "Copy"}
              </button>
            </div>
            {error ? (
              <p className="small red" role="alert">
                {error}
              </p>
            ) : (
              <p className="small muted">Works once, within 15 minutes.</p>
            )}
          </li>
          <li>
            Remove the old route (<span className="soft">{guide.oldAddress}</span>).
          </li>
        </ol>
        <div className="dialog-foot">
          <button className="btn text muted" onClick={() => close(true)}>
            Don't show again
          </button>
          <button className="btn md primary" onClick={() => close(true)}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
