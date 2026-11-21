import { useId, useState } from "react";
import type { FormEvent } from "react";
import type { createSandbox } from "./api";
import { adeName } from "./ades";
import type { Mode } from "./ades";
import { thisComputer } from "./common";
import type { Run } from "./common";
import Progress from "./Progress";
import type { AdeId, AppState, SleepChoice } from "./types";

export type CreateArgs = Parameters<typeof createSandbox>[0];

/** The server's default idle time (30 min), else the first choice that sleeps at all. */
export function defaultSleep(choices: SleepChoice[]): number {
  if (choices.some((c) => c.seconds === 1800)) return 1800;
  return choices.find((c) => c.seconds !== 0)?.seconds ?? 1800;
}

const FREE_SIZE = "4 vCPU / 8 GB";

/** What the create sets up, by mode. */
function setupText(state: AppState, mode: Mode): string | null {
  if (mode === "instabox") return null;
  if (mode === "t3") {
    return `Installs T3${state.t3 ? ` ${state.t3}` : ""}, the same version as T3 Code on ${thisComputer()}. Then the sandbox opens in T3 Code.`;
  }
  const name = adeName(state, mode);
  return `Sets the sandbox up for ${name}. Then it opens in ${name}.`;
}

interface Props {
  state: AppState;
  run: Run;
  /** The ADE the create sets up; plain Instabox sets none up. */
  mode: Mode;
  onConnect: (id: string, ade: AdeId) => void;
  /** Starts the create; the app returns to the list. */
  onCreate: (args: CreateArgs, sizeLabel: string) => void;
  onBack: () => void;
}

const cols = (n: number) => ({ gridTemplateColumns: `repeat(${n <= 3 ? Math.max(n, 1) : 2}, minmax(0, 1fr))` });

export default function NewSandbox({ state, run, mode, onConnect, onCreate, onBack }: Props) {
  const info = setupText(state, mode);
  const free = state.tier === "free";
  const fromId = useId();
  const sizeId = useId();
  const [name, setName] = useState("");
  const [fromSnapshot, setFromSnapshot] = useState(false);
  // What the user picked. Until then (or once it leaves the list) the default applies:
  // the first ready snapshot, the plan's first size, the default auto sleep.
  const [pickedSnapshot, setPickedSnapshot] = useState("");
  const [pickedSize, setPickedSize] = useState("");
  const [pickedSleep, setPickedSleep] = useState<number | null>(null);

  const snapshotId = state.snapshots.some((s) => s.id === pickedSnapshot) ? pickedSnapshot : (state.snapshots[0]?.id ?? "");
  const size = state.sizes.find((z) => z.label === pickedSize) ?? state.sizes[0];
  const sleep =
    pickedSleep !== null && state.sleepChoices.some((c) => c.seconds === pickedSleep) ? pickedSleep : defaultSleep(state.sleepChoices);
  const working = state.progress !== null && state.progress.step !== "done" && !state.progress.error;
  // A failed create left a sandbox: offer only Retry on it (or Dismiss), not a second one.
  const failedWithSandbox = !!state.progress?.machineId && !!state.progress.error;
  const canCreate =
    name.trim() !== "" &&
    !working &&
    !failedWithSandbox &&
    (free || size !== undefined) &&
    (!fromSnapshot || snapshotId !== "");

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!canCreate) return;
    // The free plan fixes size and sleep, so the app sends the plan's values.
    onCreate(
      {
        name: name.trim(),
        snapshotId: fromSnapshot ? snapshotId : undefined,
        vcpus: free ? 4 : size.vcpus,
        memMiB: free ? 8192 : size.memMiB,
        autoSuspendAfterSeconds: free ? 1800 : sleep,
        ade: mode === "instabox" ? undefined : mode,
      },
      free ? FREE_SIZE : size.label,
    );
  };

  return (
    <form className="view-form" onSubmit={submit}>
      <div className="scroll">
        <div className="stack">
          <div className="field">
            <label htmlFor="sb-name" className="section-label">
              Name
            </label>
            <input id="sb-name" className="input" value={name} onChange={(e) => setName(e.target.value)} placeholder="dev-2" autoComplete="off" />
          </div>

          <div className="field">
            <span id={fromId} className="section-label">
              Start from
            </span>
            <div className="seg" role="group" aria-labelledby={fromId} style={cols(2)}>
              <button type="button" aria-pressed={!fromSnapshot} onClick={() => setFromSnapshot(false)}>
                Standard Linux
              </button>
              <button type="button" aria-pressed={fromSnapshot} disabled={state.snapshots.length === 0} onClick={() => setFromSnapshot(true)}>
                My snapshot
              </button>
            </div>
            {fromSnapshot && (
              <select aria-label="Snapshot" className="select wide" value={snapshotId} onChange={(e) => setPickedSnapshot(e.target.value)}>
                {state.snapshots.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name}
                  </option>
                ))}
              </select>
            )}
          </div>

          <div className="field">
            <span id={sizeId} className="section-label">
              Size
            </span>
            <div className="seg" role="group" aria-labelledby={sizeId} style={cols(free ? 1 : state.sizes.length)}>
              {free ? (
                <button type="button" aria-pressed={true} disabled>
                  {FREE_SIZE}
                </button>
              ) : (
                state.sizes.map((z) => (
                  <button type="button" key={z.label} aria-pressed={z.label === size?.label} onClick={() => setPickedSize(z.label)}>
                    {z.label}
                  </button>
                ))
              )}
            </div>
          </div>

          <div className="field">
            <div className="inline-field">
              <label htmlFor="sb-sleep" className="section-label">
                Auto sleep
              </label>
              <select
                id="sb-sleep"
                className="select"
                disabled={free}
                value={free ? "free" : sleep}
                onChange={(e) => setPickedSleep(Number(e.target.value))}
              >
                {free ? (
                  <option value="free">After 30 min</option>
                ) : (
                  state.sleepChoices.map((s) => (
                    <option key={s.seconds} value={s.seconds}>
                      {s.label}
                    </option>
                  ))
                )}
              </select>
            </div>
          </div>

          {info && <p className="info">{info}</p>}

          {state.progress && (
            <div className="field">
              <h2 className="section-label">Setting up a sandbox</h2>
              <Progress state={state} run={run} mode={mode} onConnect={onConnect} />
            </div>
          )}
        </div>
      </div>
      <div className="foot actions">
        <button type="button" className="btn md" onClick={onBack}>
          Cancel
        </button>
        <button type="submit" className="btn md primary" disabled={!canCreate}>
          Create sandbox
        </button>
      </div>
    </form>
  );
}
