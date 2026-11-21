import "@fontsource-variable/inter";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { isBrowserPreview } from "./api";
import "./styles.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {isBrowserPreview ? (
      <div className="browser-preview">
        <p className="preview-note">Browser preview · Sample data<br />UI changes reload automatically. Refresh to reset.</p>
        <App showHero={new URLSearchParams(window.location.search).get("layout") === "hero"} />
      </div>
    ) : <App />}
  </StrictMode>,
);
