import { signIn } from "./api";
import type { Run } from "./common";
import type { AppState } from "./types";
import wordmark from "./assets/sign-in/instabox-wordmark.svg";
import symbol from "./assets/sign-in/instabox-symbol.svg";
import github from "./assets/sign-in/github.svg";
import google from "./assets/sign-in/google.svg";

/** Sign-in. An ADE is never asked for here: it is offered after sign-in, in the list. */
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
          <span className="sign-in-provider-label">
            <img src={github} alt="" />
            Sign in with GitHub
          </span>
        </button>
        <button className="btn primary sign-in-provider" onClick={() => run(() => signIn("google"))}>
          <span className="sign-in-provider-label">
            <img src={google} alt="" />
            Sign in with Google
          </span>
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
