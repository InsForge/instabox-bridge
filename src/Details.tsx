import { useEffect, useId, useState } from "react";
import {
  agentSignIn,
  agentStatus,
  deleteSandbox,
  openInT3,
  openSsh,
  pairAgain,
  pause,
  reinstallT3,
  resize,
  resume,
  setAutoSleep,
  sshCommand,
  T3_DOWNLOAD_URL,
  toErrorView,
} from "./api";
import type { Agent } from "./api";
import { AGENTS, agentKey, copyText, isAsleep, isFree, Line, openExternal, StateDot, stateLabel, thisComputer } from "./common";
import type { AgentCache, OnAgentStatus, Run } from "./common";
import { Check, Copy, Terminal } from "./icons";
import Progress from "./Progress";
import type { AppState, SandboxView } from "./types";
import claudeLogo from "./assets/agents/claude-code.svg";
import codexLogo from "./assets/agents/codex.svg";

const agentLogos: Record<Agent, string> = { claude: claudeLogo, codex: codexLogo };

interface Props {
  state: AppState;
  sandbox: SandboxView;
  run: Run;
  agents: AgentCache;
  onAgentStatus: OnAgentStatus;
  onConnect: (id: string) => void;
}

/** How the sandbox's T3 compares with T3 Code on this computer. */
export function t3Relation(s: SandboxView, desktop: string | null): string | null {
  if (s.desktopBehind) return `Newer than ${thisComputer()}`;
  if (s.updatePending) return `Older than ${thisComputer()}`;
  if (s.t3Version && s.t3Version === desktop) return `Same as ${thisComputer()}`;
  return null;
}

interface AgentRowProps {
  id: string;
  running: boolean;
  agent: Agent;
  label: string;
  signedIn: boolean | undefined;
  run: Run;
  onStatus: OnAgentStatus;
}

function AgentRow({ id, running, agent, label, signedIn, run, onStatus }: AgentRowProps) {
  const check = () =>
    // One check per open or sign-in. It runs a command on the sandbox, so it never polls.
    agentStatus(id, agent).then(
      (v) => onStatus(id, agent, v),
      () => onStatus(id, agent, false),
    );
  useEffect(() => {
    if (running) void check();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, running, agent]);
  return (
    <div className="group-row">
      <span className="agent-label"><img src={agentLogos[agent]} alt="" aria-hidden="true" />{label}</span>
      {signedIn ? (
        <span className="ok">
          <Check />
          Signed in
        </span>
      ) : (
        <button className="btn primary" disabled={!running} onClick={() => run(() => agentSignIn(id, agent).then(check))}>
          Sign in
        </button>
      )}
    </div>
  );
}

/** The SSH box: nothing asked yet, the login command, or why it could not be made. */
type Ssh = { command: string } | { error: string } | null;

export default function Details({ state, sandbox: s, run, agents, onAgentStatus, onConnect }: Props) {
  const free = isFree(s, state.tier);
  const running = s.state === "running";
  const sizeId = useId();
  const sleepId = useId();
  const [ssh, setSsh] = useState<Ssh>(null);
  const [copied, setCopied] = useState(false);
  // Resize starts at the sandbox's own size, so Apply shows only once another is picked.
  const [newSize, setNewSize] = useState(s.size);
  // The size just applied, until the sandbox reports it.
  const [applied, setApplied] = useState<string | null>(null);
  useEffect(() => {
    setNewSize(s.size);
    setApplied(null);
  }, [s.size]);
  const resizePending = newSize !== s.size && newSize !== applied && !free;
  // The auto sleep just picked, until the command settles.
  const [sleepPick, setSleepPick] = useState<number | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const progress = state.progress?.machineId === s.id ? state.progress : null;

  // Asked only when the user asks: each new command may mint a 30-day token.
  const fetchSsh = () =>
    sshCommand(s.id).then(
      (command) => {
        setSsh({ command });
        return command;
      },
      (e) => {
        setSsh({ error: toErrorView(e).message });
        throw e;
      },
    );
  const copy = () => {
    const text = ssh && "command" in ssh ? Promise.resolve(ssh.command) : fetchSsh();
    copyText(text).then(
      () => setCopied(true),
      () => {},
    );
  };

  const sleepKnown = state.sleepChoices.some((c) => c.seconds === s.autoSleep.seconds);
  const sleepValue = sleepPick !== null ? String(sleepPick) : sleepKnown ? String(s.autoSleep.seconds) : "current";
  const pickSleep = (v: number) => {
    setSleepPick(v);
    void run(() => setAutoSleep(s.id, v)).then(() => setSleepPick(null));
  };
  const relation = t3Relation(s, state.t3);

  return (
    <>
      <div className="scroll">
        <div className="stack">
          <div className="card">
            <div className="card-head">
              <StateDot state={s.state} />
              <span className="strong grow">{stateLabel(s.state)}</span>
              <span className="small muted">{s.size}</span>
            </div>
            <div className="grid2">
              <div className="fact">
                <span className="small muted">T3</span>
                <span className="strong">{s.t3Version ?? (s.connected ? "Unknown" : "Not installed")}</span>
                {relation && <span className="small muted">{relation}</span>}
              </div>
              <div className="fact">
                <span className="small muted">Auto sleep</span>
                <span className="strong">{s.autoSleep.label}</span>
                {s.autoSleep.seconds !== 0 && <span className="small muted">When idle</span>}
              </div>
            </div>
            {progress ? (
              <Progress state={state} run={run} onConnect={onConnect} />
            ) : s.updatePending && !s.desktopBehind ? (
              <Line tone="amber" text={state.t3 ? `T3 is older than ${thisComputer()}'s ${state.t3}` : `T3 is older than T3 Code on ${thisComputer()}`}>
                <button className="btn sm" onClick={() => run(() => reinstallT3(s.id))}>
                  Update
                </button>
              </Line>
            ) : s.desktopBehind ? (
              <Line tone="amber" text={`T3 Code on ${thisComputer()} is older`}>
                <button className="btn sm" onClick={() => openExternal(T3_DOWNLOAD_URL)}>
                  Download
                </button>
              </Line>
            ) : null}
          </div>

          {s.connected ? (
            <button className="btn primary lg" onClick={() => run(() => openInT3(s.id))}>
              Open in T3 Code
            </button>
          ) : (
            <button className="btn primary lg" onClick={() => onConnect(s.id)}>
              Connect to T3 Code
            </button>
          )}

          <section className="section">
            <h2 className="section-label">Power</h2>
            <div className="group">
              <div className="group-row">
                <label htmlFor={sleepId}>Auto sleep</label>
                <select id={sleepId} className="select" value={sleepValue} disabled={free} onChange={(e) => pickSleep(Number(e.target.value))}>
                  {!sleepKnown && (
                    <option value="current" disabled>
                      {s.autoSleep.label}
                    </option>
                  )}
                  {state.sleepChoices.map((c) => (
                    <option key={c.seconds} value={c.seconds}>
                      {c.label}
                    </option>
                  ))}
                </select>
              </div>
              <div className="group-row">
                <label htmlFor={sizeId}>Size</label>
                <select id={sizeId} className="select" value={newSize} disabled={free} onChange={(e) => setNewSize(e.target.value)}>
                  {!state.sizes.some((z) => z.label === s.size) && (
                    <option value={s.size} disabled>
                      {s.size}
                    </option>
                  )}
                  {state.sizes.map((z) => (
                    <option key={z.label} value={z.label}>
                      {z.label}
                    </option>
                  ))}
                </select>
              </div>
              {resizePending && (
                <div className="group-row">
                  <span className="small amber">Restarts the sandbox</span>
                  <span className="row-buttons">
                    <button className="btn" onClick={() => setNewSize(s.size)}>
                      Cancel
                    </button>
                    <button
                      className="btn primary"
                      onClick={() => {
                        const z = state.sizes.find((x) => x.label === newSize);
                        if (!z) return;
                        const before = s.size;
                        setApplied(newSize);
                        void run(() => resize(s.id, z.vcpus, z.memMiB)).then((ok) => {
                          if (ok) return;
                          setApplied(null);
                          setNewSize(before);
                        });
                      }}
                    >
                      Apply
                    </button>
                  </span>
                </div>
              )}
              {running && (
                <div className="group-row">
                  <span>Pause now</span>
                  <button className="btn" onClick={() => run(() => pause(s.id))}>
                    Pause
                  </button>
                </div>
              )}
              {isAsleep(s) && (
                <div className="group-row">
                  <span>Resume now</span>
                  <button className="btn" onClick={() => run(() => resume(s.id))}>
                    Resume
                  </button>
                </div>
              )}
            </div>
          </section>

          <section className="section">
            <h2 className="section-label">Agents</h2>
            <div className="group">
              {AGENTS.map((a) => (
                <AgentRow
                  key={a.id}
                  id={s.id}
                  running={running}
                  agent={a.id}
                  label={a.label}
                  signedIn={agents[agentKey(s.id, a.id)]}
                  run={run}
                  onStatus={onAgentStatus}
                />
              ))}
            </div>
          </section>

          <section className="section">
            <h2 className="section-label">SSH</h2>
            <div className="ssh">
              {ssh && "command" in ssh ? (
                <code className="ssh-box" title={ssh.command}>
                  <span>{ssh.command}</span>
                </code>
              ) : (
                <button className="ssh-box show" onClick={() => void fetchSsh().catch(() => {})}>
                  <span>Show command</span>
                </button>
              )}
              <button className="icon-btn boxed" aria-label={copied ? "Copied" : "Copy SSH command"} onClick={copy}>
                {copied ? <Check /> : <Copy />}
              </button>
              <button className="icon-btn boxed" aria-label="Open in Terminal" onClick={() => run(() => openSsh(s.id))}>
                <Terminal />
              </button>
            </div>
            {ssh && "error" in ssh && (
              <p className="small red" role="alert">
                {ssh.error}
              </p>
            )}
          </section>

          {s.connected && (
            <div className="pair">
              <button className="btn md" onClick={() => run(() => pairAgain(s.id))}>
                Pair again
              </button>
              {!s.desktopBehind && (
                <button className="btn md" onClick={() => run(() => reinstallT3(s.id))}>
                  Reinstall T3
                </button>
              )}
            </div>
          )}

          <hr className="delete-divider" />
          <button className="btn text red lg" onClick={() => (confirmDelete ? run(() => deleteSandbox(s.id)) : setConfirmDelete(true))}>
            {confirmDelete ? "Confirm delete" : "Delete sandbox"}
          </button>
        </div>
      </div>
    </>
  );
}
