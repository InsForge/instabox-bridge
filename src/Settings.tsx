import { useState } from "react";
import { setStartAtLogin, signOut } from "./api";
import { CheckT3Button } from "./Setup";
import { cap, openExternal, thisComputer } from "./common";
import type { Run } from "./common";
import type { AppState } from "./types";
import { ADES } from "./ade";
import { adeLogoUrl } from "./hero/ade-logo";

export default function Settings({ state, run }: { state: AppState; run: Run }) {
  const update = state.update;
  const name = state.account?.name || state.login || "Signed in";
  const email = state.account?.email || (state.login?.includes("@") ? state.login : null);
  const initials = name.trim().split(/\s+/).slice(0, 2).map(part => part[0]).join("").toUpperCase();
  const [failedAvatar, setFailedAvatar] = useState<string | null>(null);
  const avatar = state.account?.avatarUrl;

  return (
    <div className="scroll">
      <div className="stack">
        {update && (
          <div className="update">
            <span className="small soft">A new version of Instabox is ready.</span>
            <button className="btn primary" onClick={() => openExternal(update.url)}>
              Download
            </button>
          </div>
        )}

        <section className="section">
          <h2 className="section-label">Account</h2>
          <div className="group">
            <div className="group-row account-profile">
              <span className="account-avatar" aria-hidden="true">
                {avatar && avatar !== failedAvatar
                  ? <img src={avatar} alt="" referrerPolicy="no-referrer" onError={() => setFailedAvatar(avatar)} />
                  : initials}
              </span>
              <div className="account-identity">
                <span className="account-name" title={name}>{name}</span>
                <span className="account-email" title={email ?? undefined}>{email ?? "Email unavailable"}</span>
              </div>
              <button className="btn account-sign-out" onClick={() => run(() => signOut())}>
                Sign out
              </button>
            </div>
          </div>
        </section>

        <section className="section">
          <h2 className="section-label">{cap(thisComputer())}</h2>
          <div className="group">
            {ADES.map((ade) => <div className="group-row" key={ade.id}>
              <span className="settings-ade-label">
                <span className="ade-logo" data-ade={ade.id} aria-hidden="true">
                  <img src={adeLogoUrl(ade.id)} alt="" />
                </span>
                <span>{ade.name}{ade.id === "t3" && <> <span className="muted">{state.t3 ?? "Not found"}</span></>}</span>
              </span>
              {ade.id === "t3" ? <CheckT3Button run={run} /> : <span className="small muted">Not checked</span>}
            </div>)}
            <div className="group-row plain">
              <label htmlFor="login-switch">Start at login</label>
              <button
                id="login-switch"
                role="switch"
                aria-checked={state.startAtLogin}
                className="switch"
                onClick={() => run(() => setStartAtLogin(!state.startAtLogin))}
              >
                <span />
              </button>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}
