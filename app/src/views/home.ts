// Home: the on/off switch, steadiness, click lock, and the live trace.

import { api, onTrace, type Snapshot } from "../api";
import * as store from "../store";
import { TraceView } from "../trace";
import { formatShortcut, h, icon, slider, toast, toggleRow } from "../ui";
import { errorBanner, steadinessLabel, type View } from "./shared";

export function homeView(go: (view: string) => void): View {
  const s0 = store.current();

  // --- Power ---
  const powerTitle = h("div", { class: "power-title" });
  const powerSub = h("div", { class: "power-sub" });
  const power = h(
    "button",
    {
      class: "power",
      role: "switch",
      onclick: async () => {
        const on = power.getAttribute("aria-checked") !== "true";
        store.set(await api.setEnabled(on));
      },
    },
    h("span", { class: "track" }, h("span", { class: "knob" })),
    h("span", null, powerTitle, powerSub),
  );

  // --- Steadiness ---
  const tuned = h("span", { class: "chip" }, icon("check"), "Tuned by calibration");
  const steadiness = slider({
    label: "Steadiness",
    min: 0,
    max: 100,
    step: 1,
    value: s0.settings.filter.steadiness,
    format: (v) => `${Math.round(v)} · ${steadinessLabel(v)}`,
    scale: ["Lighter, quicker", "Steadier, smoother"],
    onChange: (v) => setSteadiness(v),
  });
  const presetButtons = (
    [
      ["Light", 20],
      ["Balanced", 50],
      ["Strong", 80],
    ] as const
  ).map(([label, value]) =>
    h("button", { "data-value": String(value), onclick: () => setSteadiness(value) }, label),
  );
  async function setSteadiness(v: number) {
    await store.updateSettings((s) => {
      s.filter.steadiness = v;
      s.filter.custom = null;
    });
  }

  const clickLock = toggleRow(
    "Click lock",
    "Holds the pointer still for a moment when you press, so clicks don't slip.",
    s0.settings.click.anchor_enabled,
    (on) => void store.updateSettings((s) => (s.click.anchor_enabled = on)),
  );

  const controls = h(
    "section",
    { class: "card", "aria-label": "Steadying" },
    power,
    h("div", { class: "divider" }),
    steadiness.el,
    h("div", { class: "presets", role: "group", "aria-label": "Presets" }, presetButtons),
    tuned,
    h("div", { class: "divider" }),
    clickLock.el,
  );

  // --- Live trace ---
  const trace = new TraceView();
  const meterValue = h("strong", null, "–");
  const meter = h("div", { class: "trace-meter", "aria-live": "off" }, meterValue, h("span", null, "shake removed"));
  trace.onMetric = (p) => (meterValue.textContent = p === null ? "–" : `${Math.round(p)}%`);
  const badge = h("span", { class: "pill trace-badge" });
  const empty = h("div", { class: "trace-empty" });
  const liveBtn = h("button", { onclick: () => setSource("live") }, "Live");
  const demoBtn = h("button", { onclick: () => setSource("demo") }, "Demo");
  let source: "live" | "demo" = s0.running ? "live" : "demo";
  let switching = false;

  async function setSource(next: "live" | "demo") {
    if (next === "live" && !store.current().running) {
      toast("Steadying isn't running yet — see the note above.");
      return;
    }
    source = next;
    switching = true;
    trace.clear();
    try {
      store.set(await api.setDemo(next === "demo", "moderate"));
    } finally {
      switching = false;
    }
  }

  const traceCard = h(
    "section",
    { class: "card trace-card", "aria-label": "Live trace" },
    h(
      "div",
      { class: "card-head" },
      h("div", null, h("h2", null, "Live trace"), h("p", { class: "muted small" }, "Your hand's path next to what Calmcursor shows.")),
      h("div", { class: "segmented", role: "group", "aria-label": "Trace source" }, liveBtn, demoBtn),
    ),
    h("div", { class: "trace-wrap" }, trace.canvas, empty, badge, meter),
    h(
      "div",
      { class: "legend" },
      h("span", null, h("i", { class: "raw" }), "Your hand (raw)"),
      h("span", null, h("i", { class: "smooth" }), "Steadied pointer"),
      h("span", null, h("i", { class: "click" }), "Click"),
    ),
  );

  // --- Stats ---
  const statClicks = h("div", { class: "value" });
  const statBounces = h("div", { class: "value" });
  const statMoves = h("div", { class: "value" });
  const statTremor = h("div", { class: "value" });
  const tremorLabel = h("div", { class: "label" });
  const stats = h(
    "section",
    { class: "stats", "aria-label": "This session" },
    h("div", { class: "stat" }, statClicks, h("div", { class: "label" }, "clicks held steady")),
    h("div", { class: "stat" }, statBounces, h("div", { class: "label" }, "accidental presses stopped")),
    h("div", { class: "stat" }, statMoves, h("div", { class: "label" }, "movements smoothed")),
    h(
      "button",
      { class: "stat", style: { cursor: "pointer", textAlign: "left", font: "inherit" }, onclick: () => go("calibrate") },
      statTremor,
      tremorLabel,
    ),
  );

  const bannerSlot = h("div");
  const statusPill = h("span", { class: "pill" });

  const el = h(
    "div",
    { class: "view-inner" },
    h(
      "header",
      { class: "view-head" },
      h("div", null, h("h1", null, "Steady pointer"), h("p", null, "Calmcursor smooths out hand tremor so the pointer goes where you mean it to.")),
      statusPill,
    ),
    bannerSlot,
    h("div", { class: "grid-2" }, controls, traceCard),
    stats,
  );

  let unlisten: (() => void) | undefined;
  void onTrace((p) => {
    if (p.source !== source) return;
    trace.push(p.samples);
    empty.hidden = trace.hasData;
  }).then((u) => (unlisten = u));
  trace.start();
  void api.setTrace(true);
  if (source === "demo") void setSource("demo");

  function update(s: Snapshot) {
    const on = s.enabled && s.running && !s.calibrating;
    power.setAttribute("aria-checked", String(s.enabled));
    powerTitle.textContent = s.enabled ? "Steadying is on" : "Steadying is paused";
    powerSub.textContent = s.settings.pause_shortcut
      ? `Pause or resume any time with ${formatShortcut(s.settings.pause_shortcut, s.os)}`
      : "Turn it off and on here or from the tray icon.";

    statusPill.className = `pill ${!s.running ? "warn" : on ? "on" : "off"}`;
    statusPill.replaceChildren(
      h("span", { class: "dot" }),
      s.calibrating ? "Calibrating" : !s.running ? "Not running" : s.enabled ? "Active" : "Paused",
    );

    steadiness.set(s.settings.filter.steadiness);
    tuned.hidden = s.settings.filter.custom === null;
    for (const b of presetButtons) {
      const v = Number(b.dataset.value);
      b.setAttribute(
        "aria-pressed",
        String(s.settings.filter.custom === null && Math.round(s.settings.filter.steadiness) === v),
      );
    }
    clickLock.set(s.settings.click.anchor_enabled);

    if (source === "live" && !s.running) source = "demo";
    if (source === "demo" && !s.demo && !switching) void setSource("demo");
    liveBtn.setAttribute("aria-pressed", String(source === "live"));
    demoBtn.setAttribute("aria-pressed", String(source === "demo"));
    liveBtn.disabled = !s.running;
    badge.className = `pill trace-badge ${source === "live" ? "on" : ""}`;
    badge.replaceChildren(
      h("span", { class: "dot" }),
      source === "live" ? s.backend.split(" (")[0] : "Demo: simulated tremor",
    );
    empty.replaceChildren(
      source === "live"
        ? h("p", null, "Move your mouse to see your path here.")
        : h("p", null, "Starting the demo…"),
    );
    empty.hidden = trace.hasData;

    statClicks.textContent = s.stats.steadied_clicks.toLocaleString();
    statBounces.textContent = s.stats.suppressed_presses.toLocaleString();
    statMoves.textContent = compact(s.stats.motion_events);
    const c = s.settings.calibration;
    statTremor.textContent = c ? (c.frequency_hz ? `${c.frequency_hz.toFixed(1)} Hz` : "Steady") : "–";
    tremorLabel.textContent = c
      ? `your tremor, about ±${c.amplitude_px.toFixed(1)} px`
      : "not calibrated yet: calibrate now";

    bannerSlot.replaceChildren(...errorBanner(s));
  }

  return {
    el,
    update,
    destroy() {
      trace.stop();
      unlisten?.();
      void api.setTrace(false);
      void api.setDemo(false);
    },
  };
}

function compact(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 10_000) return `${Math.round(n / 1000)}k`;
  return n.toLocaleString();
}
