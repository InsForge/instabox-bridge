import { useState } from "react";
import type { MouseEvent, ReactNode } from "react";
import { agentSignIn, isBrowserPreview, openUrl, pause, reinstallT3, resume, switchOrg, T3_DOWNLOAD_URL } from "./api";
import type { Agent } from "./api";
import { AGENTS, agentKey, BUSY, cap, isAsleep, Line, openExternal, StateDot, stateLabel, thisComputer } from "./common";
import type { AgentCache, OnAgentStatus, Run } from "./common";
import { Alert, Dots, ExternalLink, Gear, Plus } from "./icons";
import pauseIcon from "./assets/main-view/pause.svg";
import playIcon from "./assets/main-view/play.svg";
import unfold from "./assets/main-view/unfold.svg";
import Progress, { ProgressBar, stepText } from "./Progress";
import { T3Missing } from "./Setup";
import type { AppState, SandboxView } from "./types";
import { adeName } from "./ade";
import type { Ade, AdeConnection } from "./ade";
import SelectMenu from "./SelectMenu";

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
  | { kind: "connect" }
  | { kind: "agent"; agent: Agent; label: string }
  | null;

export function rowLine(s: SandboxView, state: AppState, agents: AgentCache): RowLine {
  if (state.progress && state.progress.machineId === s.id) return { kind: "progress" };
  // A sandbox ahead of this computer never offers Update.
  if (s.updatePending && !s.desktopBehind) return { kind: "update" };
  if (s.desktopBehind) return { kind: "desktopBehind" };
  if (!s.connected) return { kind: "connect" };
  if (s.state === "running") {
    const a = AGENTS.find((x) => agents[agentKey(s.id, x.id)] === false);
    if (a) return { kind: "agent", agent: a.id, label: a.label };
  }
  return null;
}

interface RowProps {
  ade: Ade;
  connection?: AdeConnection;
  s: SandboxView;
  state: AppState;
  run: Run;
  agents: AgentCache;
  menuOpen: boolean;
  onConnect: (id: string) => void;
  onAdeConnect: (id: string) => void;
  onDetails: (id: string) => void;
  onMenu: (id: string, anchor: HTMLElement) => void;
  onAgentStatus: OnAgentStatus;
}

function Row({ s, state, run, agents, menuOpen, onConnect, onDetails, onMenu, onAgentStatus, onAdeConnect, ade, connection }: RowProps) {
  const showAdeConnection = connection === "checking" || connection === "connecting" || ade !== "t3";
  const line = showAdeConnection ? null : rowLine(s, state, agents);
  const [powerPending, setPowerPending] = useState(false);
  const powerLabel = s.state === "running" ? "Pause" : "Resume";
  const changePower = () => {
    if (powerPending) return;
    setPowerPending(true);
    void run(() => s.state === "running" ? pause(s.id) : resume(s.id)).finally(() => setPowerPending(false));
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
      {connection && connection !== "connected" && (showAdeConnection || !line) && (
        <div className="line ade-connection" data-status={connection} role="status" title="Simulated connection check">
          <span>{connection === "checking" ? `Checking ${adeName(ade)} connection…` : connection === "connecting" ? `Connecting to ${adeName(ade)}…` : `Not connected to ${adeName(ade)}`}</span>
          {connection === "disconnected" && <button className="btn sm" disabled={BUSY.includes(s.state)}
            onClick={() => ade === "t3" ? onConnect(s.id) : onAdeConnect(s.id)}>Connect</button>}
        </div>
      )}
      {line?.kind === "progress" && <Progress state={state} run={run} onConnect={onConnect} />}
      {line?.kind === "update" && (
        <Line tone="amber" text={state.t3 ? `T3 is older than ${thisComputer()}'s ${state.t3}` : `T3 is older than T3 Code on ${thisComputer()}`}>
          <button className="btn sm" onClick={() => run(() => reinstallT3(s.id))}>
            Update
          </button>
        </Line>
      )}
      {line?.kind === "desktopBehind" && (
        <Line tone="amber" text={`T3 Code on ${thisComputer()} is older`}>
          <button className="btn sm" onClick={() => openExternal(T3_DOWNLOAD_URL)}>
            Download
          </button>
        </Line>
      )}
      {line?.kind === "connect" && (
        <Line text="Not in T3 Code yet">
          <button className="btn sm" disabled={BUSY.includes(s.state)} onClick={() => onConnect(s.id)}>
            Connect
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
function PendingRow({ state, run, pending, onConnect }: { state: AppState; run: Run; pending: PendingCreate | null; onConnect: (id: string) => void }) {
  const p = state.progress!;
  const name = p.kind === "create" && pending ? pending.name : "New sandbox";
  const size = p.kind === "create" ? pending?.size : undefined;
  const meta = p.error ? size : [size, stepText(p, state.t3)].filter(Boolean).join(" · ");
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
      {p.error && <Progress state={state} run={run} onConnect={onConnect} />}
    </li>
  );
}

interface Props {
  header: ReactNode;
  ade: Ade;
  adeMissing: boolean;
  adeConnections: Record<string, AdeConnection> | null;
  state: AppState;
  run: Run;
  agents: AgentCache;
  pending: PendingCreate | null;
  menuFor: string | null;
  onNew: () => void;
  onSettings: () => void;
  onConnect: (id: string) => void;
  onAdeConnect: (id: string) => void;
  onDetails: (id: string) => void;
  onMenu: (id: string, anchor: HTMLElement) => void;
  onAgentStatus: OnAgentStatus;
}

export default function SandboxList({ header, state, run, agents, pending, menuFor, onNew, onSettings, onConnect, onDetails, onMenu, onAgentStatus, onAdeConnect, ade, adeMissing, adeConnections }: Props) {
  const p = state.progress;
  const showPending = p !== null && !state.sandboxes.some((s) => s.id === p.machineId);
  const empty = state.sandboxes.length === 0 && !showPending;
  const organization = state.orgs.find((o) => o.id === state.orgId);
  const workspaceName = organization?.name ?? state.login ?? "Workspace";
  return (
    <>
      <div className="scroll main-scroll">
        {header}
        {!empty && <div className="list-head">
          <div className="list-heading">
            <h1 className="list-title">
              Sandboxes <span className="muted">{state.sandboxes.length}</span>
            </h1>
          </div>
          <button className="btn primary" onClick={onNew}>
            <Plus />
            New sandbox
          </button>
        </div>}
        {ade === "t3" && state.t3 === null && (
          <div className="list-card">
            <T3Missing run={run} />
          </div>
        )}
        {empty ? (
          <div className="empty">
            <div className="empty-copy">
              <h1 className="list-title">No sandboxes</h1>
              <p>Create one and open it in {adeName(ade)}.</p>
            </div>
            <button className="btn primary" onClick={onNew}>
              <Plus />
              New sandbox
            </button>
          </div>
        ) : (
          <ul className="rows">
            {showPending && <PendingRow state={state} run={run} pending={pending} onConnect={onConnect} />}
            {state.sandboxes.map((s) => (
              <Row
                key={s.id}
                ade={ade}
                connection={adeConnections?.[s.id]}
                s={s}
                state={state}
                run={run}
                agents={agents}
                menuOpen={menuFor === s.id}
                onConnect={onConnect}
                onAdeConnect={onAdeConnect}
                onDetails={onDetails}
                onMenu={onMenu}
                onAgentStatus={onAgentStatus}
              />
            ))}
          </ul>
        )}
      </div>
      {adeMissing && (
        <div className="ade-install-notice">
          <div className="warn-card" role="status">
            <div className="warn-head">
              <span className="warn-icon"><Alert /></span>
              <div className="warn-text">
                <span className="strong">{adeName(ade)} is not installed on {thisComputer()}</span>
                <span className="small soft">Install {adeName(ade)} to connect your sandboxes.</span>
              </div>
            </div>
            <div className="warn-buttons">
              <button className="btn primary" onClick={() => {
                if (isBrowserPreview) window.open("https://cmux.com", "_blank", "noopener,noreferrer");
                else void run(() => openUrl("https://cmux.com"));
              }}>
                Install <ExternalLink />
              </button>
            </div>
          </div>
        </div>
      )}
      <div className="foot workspace-footer">
        <SelectMenu className="workspace-picker" label="Workspace" value={state.orgId ?? ""} above
          disabled={state.orgs.length < 2} onChange={(value) => { void run(() => switchOrg(value)); }}
          options={state.orgs.map((org) => ({ value: org.id, label: org.name,
            content: <><span className="plan-badge">{cap(org.tier)}</span><span className="selection-label">{org.name}</span></> }))}>
          <span className="workspace-name">{workspaceName}</span>
          <img src={unfold} alt="" />
        </SelectMenu>
        <div className="workspace-actions">
          <button className="icon-btn hd" aria-label="Open InstaBox dashboard" title="Open dashboard in browser"
            onClick={() => {
              if (isBrowserPreview) window.open(DASHBOARD_URL, "_blank", "noopener,noreferrer");
              else openExternal(DASHBOARD_URL);
            }}>
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
