import { useEffect, useState } from "react";
import { refreshT3, signIn, T3_DOWNLOAD_URL } from "./api";
import { openExternal, thisComputer } from "./common";
import type { Run } from "./common";
import { Alert } from "./icons";
import type { AppState } from "./types";
import wordmark from "./assets/sign-in/instabox-wordmark.svg";
import symbol from "./assets/sign-in/instabox-symbol.svg";
import github from "./assets/sign-in/github.svg";
import google from "./assets/sign-in/google.svg";

/** Reads the T3 Code version on this computer again, and shows that it did. */
export function CheckT3Button({ run }: { run: Run }) {
  const [phase, setPhase] = useState<"idle" | "checking" | "done">("idle");
  useEffect(() => {
    if (phase !== "done") return;
    const t = setTimeout(() => setPhase("idle"), 1500);
    return () => clearTimeout(t);
  }, [phase]);
  const check = () => {
    setPhase("checking");
    void run(() => refreshT3()).then((ok) => setPhase(ok ? "done" : "idle"));
  };
  return (
    <button className="btn check-t3" disabled={phase === "checking"} onClick={check}>
      {phase === "checking" ? "Checking…" : phase === "done" ? "Checked" : "Check again"}
    </button>
  );
}

/** T3 Code is not on this computer: shown after sign-in, at the top of the list. */
export function T3Missing({ run }: { run: Run }) {
  return (
    <div className="warn-card">
      <div className="warn-head">
        <span className="warn-icon">
          <Alert />
        </span>
        <div className="warn-text">
          <span className="strong">T3 Code is not on {thisComputer()}</span>
          <span className="small soft">Instabox opens sandboxes in T3 Code. Install it first.</span>
        </div>
      </div>
      <div className="warn-buttons">
        <CheckT3Button run={run} />
        <button className="btn primary" onClick={() => openExternal(T3_DOWNLOAD_URL)}>
          Download T3 Code
        </button>
      </div>
    </div>
  );
}

export default function Setup({ state, run }: { state: Pick<AppState, "signInCode">; run: Run }) {
  return (
    <div className="setup">
      <div className="brand-block">
        <div className="sign-in-logo" role="img" aria-label="InstaBox">
          <img className="sign-in-symbol" src={symbol} alt="" />
          <img className="sign-in-wordmark" src={wordmark} alt="" />
        </div>
        <p className="muted">Run any ADE on an InstaBox Sandbox.</p>
      </div>
      <div className="sign-in">
        <button className="btn primary sign-in-provider" onClick={() => run(() => signIn("github"))}>
          <span className="sign-in-provider-label"><img src={github} alt="" />Sign in with GitHub</span>
        </button>
        <button className="btn primary sign-in-provider" onClick={() => run(() => signIn("google"))}>
          <span className="sign-in-provider-label"><img src={google} alt="" />Sign in with Google</span>
        </button>
        {state.signInCode && (
          <p className="sign-in-code" role="status">
            Waiting for sign-in in your browser. Your code is <strong className="code">{state.signInCode}</strong>
          </p>
        )}
      </div>
    </div>
  );
}
