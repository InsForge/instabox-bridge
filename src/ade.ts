import { useEffect, useRef, useState } from "react";
import type { AppState } from "./types";

export const ADES = [
  { id: "t3", name: "T3 Code", initials: "T3" },
  { id: "orca", name: "Orca", initials: "O" },
  { id: "cmux", name: "cmux", initials: "cm" },
  { id: "paradev", name: "Paradev", initials: "P" },
] as const;

export type Ade = (typeof ADES)[number]["id"];
export type AdeConnection = "checking" | "connecting" | "connected" | "disconnected";
export const adeName = (ade: Ade) => ADES.find((item) => item.id === ade)!.name;

type MachineConnection = { id: string; connected: boolean };

/** Replace this simulation with the ADE connection API when it is available. */
function simulateConnections(ade: Ade, machines: MachineConnection[]): Record<string, AdeConnection> {
  return Object.fromEntries(machines.map((machine) => {
    // Stable per-machine examples; changing list order does not change the result.
    const seed = [...machine.id].reduce((sum, char) => sum + char.charCodeAt(0), 0);
    const offset = { t3: 0, orca: 2, cmux: 1, paradev: 0 }[ade];
    const connected = ade === "t3" ? machine.connected : (seed + offset) % 3 === 0;
    return [machine.id, connected ? "connected" : "disconnected"];
  }));
}

export function useAdeConnections(ade: Ade, state: AppState | null, request: number) {
  const [result, setResult] = useState<{ key: string; connections: Record<string, AdeConnection> } | null>(null);
  const connected = useRef(new Set<string>());
  const pending = useRef(new Map<string, ReturnType<typeof setTimeout>>());
  const scope = JSON.stringify([state?.login, state?.orgId, ade]);
  const connectionKey = (id: string) => JSON.stringify([scope, id]);
  // Scope results to both the account and workspace, and recheck changed machine sets.
  const machines = JSON.stringify(state?.sandboxes.map(({ id, connected }) => ({ id, connected })).sort((a, b) => a.id.localeCompare(b.id)) ?? []);
  // cmux demonstrates a missing Mac app in the native app and browser preview.
  const missing = ade === "cmux";
  const active = request > 0 && !!state?.signedIn && !missing;
  const key = JSON.stringify([request, active, ade, state?.login, state?.orgId, machines]);

  useEffect(() => {
    if (!active) return;
    const timer = setTimeout(() => {
      const connections = simulateConnections(ade, JSON.parse(machines) as MachineConnection[]);
      if (ade !== "t3") {
        for (const id of Object.keys(connections)) {
          if (connected.current.has(JSON.stringify([scope, id]))) connections[id] = "connected";
        }
      }
      setResult({ key, connections });
    }, 450);
    return () => {
      clearTimeout(timer);
      for (const timeout of pending.current.values()) clearTimeout(timeout);
      pending.current.clear();
    };
  }, [active, ade, key, machines, scope]);

  const connect = (id: string) => {
    if (!active || ade === "t3" || result?.key !== key || result.connections[id] !== "disconnected" || pending.current.has(id)) return;
    setResult(previous => previous?.key === key ? { key, connections: { ...previous.connections, [id]: "connecting" } } : previous);
    const idKey = connectionKey(id);
    pending.current.set(id, setTimeout(() => {
      pending.current.delete(id);
      connected.current.add(idKey);
      setResult(previous => previous?.key === key ? { key, connections: { ...previous.connections, [id]: "connected" } } : previous);
    }, 650));
  };
  // Never show the previous ADE's result, even for the render before the effect runs.
  const connections: Record<string, AdeConnection> | null = !active ? null : result?.key === key ? result.connections :
    Object.fromEntries((state?.sandboxes ?? []).map((s) => [s.id, "checking"]));
  return { connections, connect, missing };
}
