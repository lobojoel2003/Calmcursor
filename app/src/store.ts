// The latest snapshot of the app's state, shared by all screens.

import { api, onStateChanged, type Snapshot } from "./api";

type Listener = (s: Snapshot) => void;

let snapshot: Snapshot | null = null;
const listeners = new Set<Listener>();

export function current(): Snapshot {
  if (!snapshot) throw new Error("state not loaded yet");
  return snapshot;
}

export function set(s: Snapshot) {
  snapshot = s;
  for (const l of listeners) l(s);
}

export function subscribe(l: Listener): () => void {
  listeners.add(l);
  if (snapshot) l(snapshot);
  return () => listeners.delete(l);
}

export async function refresh(): Promise<Snapshot> {
  const s = await api.getState();
  set(s);
  return s;
}

export async function init() {
  await onStateChanged(set);
  await refresh();
  // Keep counters fresh while the window is open.
  window.setInterval(() => {
    if (document.visibilityState === "visible") void refresh();
  }, 1500);
}

/** Saves a modified copy of the settings. */
export async function updateSettings(mutate: (s: Snapshot["settings"]) => void) {
  const next = structuredClone(current().settings);
  mutate(next);
  set(await api.saveSettings(next));
}
