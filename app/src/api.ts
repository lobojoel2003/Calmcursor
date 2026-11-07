// Typed wrappers around the Rust commands and events.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface OneEuroParams {
  min_cutoff: number;
  beta: number;
  d_cutoff: number;
}

export interface FilterSettings {
  steadiness: number;
  custom: OneEuroParams | null;
}

export interface ClickSettings {
  anchor_enabled: boolean;
  anchor_radius_px: number;
  anchor_ms: number;
  debounce_ms: number;
}

export type Severity = "minimal" | "mild" | "moderate" | "strong";

export interface CalibrationSummary {
  date_unix: number;
  frequency_hz: number | null;
  amplitude_px: number;
  severity: Severity;
  reduction_percent: number;
  lag_ms: number;
}

export interface Settings {
  version: number;
  enabled: boolean;
  filter: FilterSettings;
  click: ClickSettings;
  tick_hz: number;
  launch_at_login: boolean;
  start_minimized: boolean;
  pause_shortcut: string;
  calibration: CalibrationSummary | null;
}

export type PlatformError =
  | { kind: "permission_denied"; message: string; help: string }
  | { kind: "no_devices"; help: string }
  | { kind: "unsupported" }
  | { kind: "os"; message: string };

export type PermissionStatus =
  | { state: "granted" }
  | { state: "needed"; message: string; help: string }
  | { state: "unknown" };

export interface EngineStats {
  motion_events: number;
  ticks: number;
  clicks: number;
  steadied_clicks: number;
  suppressed_presses: number;
}

export interface BuildInfo {
  version: string;
  build_number: string;
  git_sha: string;
  build_date: string;
  channel: string;
  target: string;
}

export interface Snapshot {
  settings: Settings;
  effective_filter: OneEuroParams;
  running: boolean;
  enabled: boolean;
  backend: string;
  devices: string[];
  warning: string | null;
  error: PlatformError | null;
  permission: PermissionStatus;
  permission_url: string | null;
  stats: EngineStats;
  build: BuildInfo;
  os: string;
  settings_path: string | null;
  calibrating: boolean;
  demo: boolean;
  shortcut_error: string | null;
}

/** [t, raw x, raw y, smoothed x, smoothed y, flags] */
export type TraceTuple = [number, number, number, number, number, number];

export interface TracePayload {
  source: "live" | "demo";
  samples: TraceTuple[];
}

export const TraceFlags = { PRESS: 1, ANCHORED: 2, SUPPRESSED: 4, BYPASS: 8 } as const;

export interface PointerSample {
  t: number;
  x: number;
  y: number;
}

export interface FollowSample extends PointerSample {
  tx: number;
  ty: number;
}

export interface TapRecord {
  down_t: number;
  down_x: number;
  down_y: number;
  up_t: number;
  up_x: number;
  up_y: number;
}

export interface PressRecord {
  t: number;
  down: boolean;
}

export interface CalibrationRecording {
  hold: PointerSample[];
  follow: FollowSample[];
  taps: TapRecord[];
  presses: PressRecord[];
}

export interface TremorProfile {
  frequency_hz: number | null;
  amplitude_px: number;
  rms_px: number;
  peak_speed_px_s: number;
  confidence: number;
  severity: Severity;
  spectrum: [number, number][];
  duration_s: number;
}

export interface Replay {
  raw: [number, number][];
  smoothed: [number, number][];
  target: [number, number][];
}

export interface CalibrationReport {
  profile: TremorProfile;
  filter: OneEuroParams;
  steadiness: number;
  click: ClickSettings;
  reduction_percent: number;
  moving_reduction_percent: number;
  lag_ms: number;
  taps: {
    count: number;
    median_drift_px: number;
    max_drift_px: number;
    median_press_ms: number;
    bounces: number;
  };
  hold_replay: Replay;
  follow_replay: Replay;
  summary: CalibrationSummary;
}

export const api = {
  getState: () => invoke<Snapshot>("get_state"),
  setEnabled: (enabled: boolean) => invoke<Snapshot>("set_enabled", { enabled }),
  saveSettings: (settings: Settings) => invoke<Snapshot>("save_settings", { settings }),
  resetSettings: () => invoke<Snapshot>("reset_settings"),
  retryEngine: () => invoke<Snapshot>("retry_engine"),
  requestPermission: () => invoke<boolean>("request_permission"),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  setTrace: (on: boolean) => invoke<void>("set_trace", { on }),
  setDemo: (on: boolean, tremor?: string) => invoke<Snapshot>("set_demo", { on, tremor }),
  beginCalibration: () => invoke<void>("begin_calibration"),
  endCalibration: () => invoke<void>("end_calibration"),
  analyzeCalibration: (recording: CalibrationRecording) =>
    invoke<CalibrationReport>("analyze_calibration", { recording }),
  applyCalibration: (r: CalibrationReport) =>
    invoke<Snapshot>("apply_calibration", {
      filter: r.filter,
      steadiness: r.steadiness,
      click: r.click,
      summary: r.summary,
    }),
  quit: () => invoke<void>("quit"),
};

export function onTrace(cb: (p: TracePayload) => void): Promise<UnlistenFn> {
  return listen<TracePayload>("trace", (e) => cb(e.payload));
}

export function onStateChanged(cb: (s: Snapshot) => void): Promise<UnlistenFn> {
  return listen<Snapshot>("state-changed", (e) => cb(e.payload));
}

export function onNavigate(cb: (view: string) => void): Promise<UnlistenFn> {
  return listen<string>("navigate", (e) => cb(e.payload));
}

export const REPO_URL = "https://github.com/lobojoel2003/Calmcursor";
