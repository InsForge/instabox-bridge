import { useState } from "react";
import type { MouseEvent, ReactNode } from "react";
import { agentSignIn, isBrowserPreview, pause, reinstallT3, resume, switchOrg, T3_DOWNLOAD_URL } from "./api";
import type { Agent } from "./api";
import { AdeMissing, ConnectAdeCard } from "./AdeCards";
import { adeName, hasAde, localAde, readyText } from "./ades";
import type { Mode } from "./ades";
import { AGENTS, agentKey, BUSY, cap, isAsleep, Line, openExternal, StateDot, stateLabel, thisComputer } from "./common";
import type { AgentCache, OnAgentStatus, Run } from "./common";
import { Dots, ExternalLink, Gear, Plus } from "./icons";
import pauseIcon from "./assets/main-view/pause.svg";
import playIcon from "./assets/main-view/play.svg";
import unfold from "./assets/main-view/unfold.svg";
import Progress, { ProgressBar, stepText } from "./Progress";
import SelectMenu from "./SelectMenu";
import type { AdeId, AppState, SandboxView } from "./types";

const DASHBOARD_URL = "https://dashboard.instabox.sh/dashboard/machines";

/** What a create that has no sandbox yet looks like: the name and size the form sent. */
export interface PendingCreate {
  name: string;
  size: string;
}

/** The one line under a row, by priority. Agent lines come only from statuses the details view fetched. */
export type RowLine =
  | { kind: "progress" }
  | { kind: "update" }
  | { kind: "desktopBehind" }
  | { kind: "open"; ade: AdeId }
  | { kind: "install"; ade: AdeId }
  | { kind: "agent"; agent: Agent; label: string }
  | null;

/** Plain Instabox shows no ADE line; an ADE mode shows Open or Install on every row. */
export function rowLine(s: SandboxView, state: AppState, agents: AgentCache, mode: Mode): RowLine {
  if (state.progress && state.progress.machineId === s.id) return { kind: "progress" };
  if (mode === "t3") {
    // A sandbox ahead of this computer never offers Update.
    if (s.updatePending && !s.desktopBehind) return { kind: "update" };
    if (s.desktopBehind) return { kind: "desktopBehind" };
  }
  if (mode !== "instabox") return { kind: hasAde(s, mode) ? "open" : "install", ade: mode };
  if (s.state === "running") {
    const a = AGENTS.find((x) => agents[agentKey(s.id, x.id)] === false);
    if (a) return { kind: "agent", agent: a.id, label: a.label };
  }
  return null;
}

/** The actions of the list and its rows, which App runs. */
export interface ListActions {
  /** Opens the sandbox in the ADE; one without it gets it first. */
  onOpen: (id: string, ade: AdeId) => void;
  /** Sets the ADE up on the sandbox, then opens it. */
  onConnect: (id: string, ade: AdeId) => void;
  onDetails: (id: string) => void;
  onMenu: (id: string, anchor: HTMLElement) => void;
  onAgentStatus: OnAgentStatus;
}

interface RowProps extends ListActions {
  s: SandboxView;
  state: AppState;
  run: Run;
  mode: Mode;
  agents: AgentCache;
  menuOpen: boolean;
}

function Row({ s, state, run, mode, agents, menuOpen, onOpen, onConnect, onDetails, onMenu, onAgentStatus }: RowProps) {
  const line = rowLine(s, state, agents, mode);
  const busy = BUSY.includes(s.state);
  const [powerPending, setPowerPending] = useState(false);
  const powerLabel = s.state === "running" ? "Pause" : "Resume";
  const changePower = () => {
    if (powerPending) return;
    setPowerPending(true);
    void run(() => (s.state === "running" ? pause(s.id) : resume(s.id))).finally(() => setPowerPending(false));
  };
  // A click anywhere on the row opens details, except on its buttons.
  const open = (e: MouseEvent) => {
    if ((e.target as HTMLElement).closest("button, a, select, input")) return;
    onDetails(s.id);
  };
  return (
    <li className="row" onClick={open}>
      <div className="row-top">
        <button className="row-open" onClick={() => onDetails(s.id)}>
          <StateDot state={s.state} />
          <span className="row-text">
            <span className="row-title">
              <span className="name">{s.name}</span>
            </span>
            <span className="row-meta">{stateLabel(s.state)}</span>
          </span>
        </button>
        <div className="row-controls">
          {(s.state === "running" || isAsleep(s)) && (
            <button className="icon-btn" aria-label={`${powerLabel} ${s.name}`} title={powerLabel} disabled={powerPending} onClick={changePower}>
              <img src={s.state === "running" ? pauseIcon : playIcon} alt="" />
            </button>
          )}
          <button
            className="icon-btn row-more"
            aria-label={`More actions for ${s.name}`}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onClick={(e) => onMenu(s.id, e.currentTarget)}
          >
            <Dots />
          </button>
        </div>
      </div>
      {line?.kind === "progress" && <Progress state={state} run={run} mode={mode} onConnect={onConnect} />}
      {line?.kind === "update" && (
        <Line tone="amber" text={state.t3 ? `T3 is older than ${thisComputer()}'s ${state.t3}` : `T3 is older than T3 Code on ${thisComputer()}`}>
          <button className="btn sm" onClick={() => run(() => reinstallT3(s.id))}>
            Update
          </button>
        </Line>
      )}
      {line?.kind === "desktopBehind" && (
        <Line tone="amber" text={`T3 Code on ${thisComputer()} is older`}>
          <button className="btn sm" onClick={() => openExternal(localAde(state, "t3")?.downloadUrl ?? T3_DOWNLOAD_URL)}>
            Download
          </button>
        </Line>
      )}
      {line?.kind === "open" && (
        <Line text={readyText(state, s, line.ade)}>
          <button className="btn sm primary" disabled={busy} onClick={() => onOpen(s.id, line.ade)}>
            Open in {adeName(state, line.ade)}
          </button>
        </Line>
      )}
      {line?.kind === "install" && (
        <Line text={`No ${adeName(state, line.ade)} here yet`}>
          <button className="btn sm accent-outline" disabled={busy} onClick={() => onConnect(s.id, line.ade)}>
            Install {adeName(state, line.ade)}
          </button>
        </Line>
      )}
      {line?.kind === "agent" && (
        <Line tone="amber" text={`${line.label} needs a sign-in`}>
          <button
            className="btn sm"
            // No status check from the list: it would run a command on the sandbox.
            onClick={() => run(() => agentSignIn(s.id, line.agent)).then((ok) => ok && onAgentStatus(s.id, line.agent, null))}
          >
            Sign in
          </button>
        </Line>
      )}
    </li>
  );
}

/** A create in flight before its sandbox is in the list. */
function PendingRow({
  state,
  run,
  mode,
  pending,
  onConnect,
}: {
  state: AppState;
  run: Run;
  mode: Mode;
  pending: PendingCreate | null;
  onConnect: (id: string, ade: AdeId) => void;
}) {
  const p = state.progress!;
  const name = p.kind === "create" && pending ? pending.name : "New sandbox";
  const size = p.kind === "create" ? pending?.size : undefined;
  const meta = p.error ? size : [size, stepText(p, state)].filter(Boolean).join(" · ");
  return (
    <li className="row pending">
      <div className="row-top">
        <div className="row-open">
          <StateDot state={p.error ? "error" : "creating"} />
          <div className="row-text">
            <span className="row-title">
              <span className="name">{name}</span>
              <span className="state">{p.error ? "Failed" : p.kind === "create" ? "Creating" : "Connecting"}</span>
            </span>
            {meta && <span className="row-meta">{meta}</span>}
            {!p.error && <ProgressBar p={p} />}
          </div>
        </div>
      </div>
      {p.error && <Progress state={state} run={run} mode={mode} onConnect={onConnect} />}
    </li>
  );
}

interface Props extends ListActions {
  /** The main header and the banners, drawn above the sandbox heading. */
  header: ReactNode;
  showHero?: boolean;
  state: AppState;
  run: Run;
  mode: Mode;
  agents: AgentCache;
  pending: PendingCreate | null;
  menuFor: string | null;
  /** Plain Instabox before any pick: offer an ADE. */
  firstRun: boolean;
  onNew: () => void;
  onSettings: () => void;
  onPickAde: () => void;
  onNotNow: () => void;
}

export default function SandboxList(props: Props) {
  const { header, showHero = false, state, run, mode, agents, pending, menuFor, firstRun } = props;
  const { onNew, onSettings, onPickAde, onNotNow, onConnect } = props;
  const p = state.progress;
  const showPending = p !== null && !state.sandboxes.some((s) => s.id === p.machineId);
  const empty = state.sandboxes.length === 0 && !showPending;
  const missing = mode !== "instabox" && localAde(state, mode)?.version == null;
  const organization = state.orgs.find((o) => o.id === state.orgId);
  const workspaceName = organization?.name ?? state.login ?? "Workspace";
  const heading = !empty && (
    <div className="list-head">
      <div className="list-heading">
        <h1 className="list-title">
          Sandboxes <span className="muted">{state.sandboxes.length}</span>
        </h1>
      </div>
      <button className="btn primary" onClick={onNew}>
        <Plus />
        New sandbox
      </button>
    </div>
  );
  return (
    <>
      {!showHero && (
        <div className="main-list-header">
          {header}
          {heading}
        </div>
      )}
      <div className={`scroll main-scroll${showHero ? "" : " main-scroll-compact"}`} data-empty={empty}>
        {showHero && (
          <>
            {header}
            {heading}
          </>
        )}
        {empty ? (
          <div className="empty">
            <div className="empty-copy">
              <h1 className="list-title">No sandboxes</h1>
              <p>{mode === "instabox" ? "Create one to get started." : `Create one and open it in ${adeName(state, mode)}.`}</p>
            </div>
            <button className="btn primary" onClick={onNew}>
              <Plus />
              New sandbox
            </button>
          </div>
        ) : (
          <ul className="rows">
            {showPending && <PendingRow state={state} run={run} mode={mode} pending={pending} onConnect={onConnect} />}
            {state.sandboxes.map((s) => (
              <Row key={s.id} {...props} s={s} agents={agents} menuOpen={menuFor === s.id} />
            ))}
          </ul>
        )}
        {mode === "instabox" && firstRun && <ConnectAdeCard state={state} onConnect={onPickAde} onNotNow={onNotNow} />}
      </div>
      {mode !== "instabox" && missing && (
        <div className="ade-install-notice">
          <AdeMissing state={state} ade={mode} run={run} />
        </div>
      )}
      <div className="foot workspace-footer">
        <SelectMenu
          className="workspace-picker"
          label="Workspace"
          value={state.orgId ?? ""}
          above
          disabled={state.orgs.length < 2}
          onChange={(value) => {
            void run(() => switchOrg(value));
          }}
          options={state.orgs.map((org) => ({
            value: org.id,
            label: org.name,
            content: (
              <>
                <span className="plan-badge">{cap(org.tier)}</span>
                <span className="selection-label">{org.name}</span>
              </>
            ),
          }))}
        >
          <span className="workspace-name">{workspaceName}</span>
          <img src={unfold} alt="" />
        </SelectMenu>
        <div className="workspace-actions">
          <button
            className="icon-btn hd"
            aria-label="Open InstaBox dashboard"
            title="Open dashboard in browser"
            onClick={() => {
              if (isBrowserPreview) window.open(DASHBOARD_URL, "_blank", "noopener,noreferrer");
              else openExternal(DASHBOARD_URL);
            }}
          >
            <ExternalLink />
          </button>
          <button className="icon-btn hd" aria-label="Settings" title={state.update ? "Update available" : undefined} onClick={onSettings}>
            <Gear />
            {state.update && <span className="badge" aria-hidden="true" />}
          </button>
        </div>
      </div>
    </>
  );
}
