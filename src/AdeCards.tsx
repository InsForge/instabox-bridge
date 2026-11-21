import { useEffect, useState } from "react";
import { refreshAdes } from "./api";
import { AdeBadge, adeName, localAde } from "./ades";
import { openExternal, thisComputer } from "./common";
import type { Run } from "./common";
import { Alert } from "./icons";
import type { AdeId, AppState } from "./types";

/** Looks for the ADEs on this computer again, and shows that it did. */
export function CheckAdesButton({ run, text }: { run: Run; text?: boolean }) {
  const [phase, setPhase] = useState<"idle" | "checking" | "done">("idle");
  useEffect(() => {
    if (phase !== "done") return;
    const t = setTimeout(() => setPhase("idle"), 1500);
    return () => clearTimeout(t);
  }, [phase]);
  const check = () => {
    setPhase("checking");
    void run(() => refreshAdes()).then((ok) => setPhase(ok ? "done" : "idle"));
  };
  return (
    <button className={`btn check-ades${text ? " text" : ""}`} disabled={phase === "checking"} onClick={check}>
      {phase === "checking" ? "Checking…" : phase === "done" ? "Checked" : "Check again"}
    </button>
  );
}

/** The selected ADE is not on this computer: shown at the top of the list. */
export function AdeMissing({ state, ade, run }: { state: AppState; ade: AdeId; run: Run }) {
  const name = adeName(state, ade);
  const url = localAde(state, ade)?.downloadUrl;
  return (
    <div className="warn-card">
      <div className="warn-head">
        <span className="warn-icon">
          <Alert />
        </span>
        <div className="warn-text">
          <span className="strong">
            {name} is not on {thisComputer()}
          </span>
          <span className="small soft">InstaBox opens sandboxes in {name}. Install it first.</span>
        </div>
      </div>
      <div className="warn-buttons">
        <CheckAdesButton run={run} />
        {url && (
          <button className="btn primary" onClick={() => openExternal(url)}>
            Download {name}
          </button>
        )}
      </div>
    </div>
  );
}

/** "A, B and C" */
const listOf = (names: string[]) => (names.length < 2 ? names.join("") : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`);

/** First run in plain Instabox: offers an ADE, never forces one. */
export function ConnectAdeCard({ state, onConnect, onNotNow }: { state: AppState; onConnect: () => void; onNotNow: () => void }) {
  const found = state.ades.filter((a) => a.version !== null).map((a) => a.name);
  const here = found.length > 0 ? `${listOf(found)} ${found.length === 1 ? "is" : "are"} on ${thisComputer()}. ` : "";
  return (
    <section className="ade-card" aria-label="Connect an ADE">
      <div className="ade-card-badges" aria-hidden="true">
        {state.ades.map((a) => (
          <AdeBadge key={a.id} id={a.id} />
        ))}
      </div>
      <div className="ade-card-text">
        <span className="strong">Open sandboxes in your ADE</span>
        <span className="small muted">
          {here}Pick one to get an Open button on every sandbox. Change it or go back to InstaBox from the menu at the top.
        </span>
      </div>
      <div className="ade-card-buttons">
        <button className="btn" aria-haspopup="menu" onClick={onConnect}>
          Connect an ADE
        </button>
        <button className="btn text muted" onClick={onNotNow}>
          Not now
        </button>
      </div>
    </section>
  );
}
