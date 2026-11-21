import { setStartAtLogin, signOut } from "./api";
import { CheckAdesButton } from "./AdeCards";
import { AdeBadge, COMING_SOON } from "./ades";
import { cap, openExternal, thisComputer } from "./common";
import type { Run } from "./common";
import type { AppState } from "./types";

/** Settings. The organization is picked in the list's footer. */
export default function Settings({ state, run }: { state: AppState; run: Run }) {
  const update = state.update;
  const name = state.login ?? "Signed in";
  const initials = name.trim().charAt(0).toUpperCase();

  return (
    <div className="scroll settings-view">
      <div className="stack">
        {update && (
          <div className="update">
            <span className="small soft">A new version of InstaBox is ready.</span>
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
                {initials}
              </span>
              <div className="account-identity">
                <span className="account-name" title={name}>
                  {name}
                </span>
              </div>
              <button className="btn account-sign-out" onClick={() => run(() => signOut())}>
                Sign out
              </button>
            </div>
          </div>
        </section>

        <section className="section">
          <div className="section-head">
            <h2 className="section-label">{cap(thisComputer())}</h2>
            <CheckAdesButton run={run} text />
          </div>
          <div className="group">
            <ul className="ade-list" aria-label={`ADEs on ${thisComputer()}`}>
              {state.ades.map((a) => (
                <li key={a.id} className="group-row">
                  <span className="settings-ade-label">
                    <AdeBadge id={a.id} />
                    <span>{a.name}</span>
                  </span>
                  <span className="small muted">{a.version ?? "Not found"}</span>
                </li>
              ))}
              {COMING_SOON.map((a) => (
                <li key={a.id} className="group-row">
                  <span className="settings-ade-label">
                    <AdeBadge id={a.id} />
                    <span className="muted">{a.name}</span>
                  </span>
                  <span className="small muted">Coming soon</span>
                </li>
              ))}
            </ul>
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
