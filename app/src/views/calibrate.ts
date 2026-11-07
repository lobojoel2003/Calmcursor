// Calibrate: the 30-second game and what it found.

import { api, type CalibrationReport, type Replay, type Snapshot } from "../api";
import { startGame } from "../game";
import * as store from "../store";
import { drawCursor } from "../trace";
import { cssVar, fitCanvas, h, icon, prefersReducedMotion, replace, toast } from "../ui";
import { formatDate, severityText, steadinessLabel, unitScale, type View } from "./shared";

/** The last report survives switching screens (not app restarts). */
let lastReport: CalibrationReport | null = null;

export function calibrateView(go: (view: string) => void): View {
  const el = h("div", { class: "view-inner" });
  let cleanup: (() => void) | undefined;

  function showIntro(error?: string) {
    cleanup?.();
    cleanup = undefined;
    const s = store.current();
    const last = s.settings.calibration;
    replace(
      el,
      h(
        "header",
        { class: "view-head" },
        h(
          "div",
          null,
          h("h1", null, "Calibrate"),
          h("p", null, "A 30-second check of your pointer. Calmcursor measures your tremor and tunes itself to you."),
        ),
      ),
      error
        ? h("div", { class: "banner", role: "alert" }, icon("alert"), h("div", { class: "body" }, h("p", null, error)))
        : null,
      h(
        "section",
        { class: "card" },
        h(
          "div",
          { class: "steps" },
          step(1, "Rest", "about 10 s", "Hold the pointer inside a circle. This shows your shake when you are still."),
          step(2, "Follow", "12 s", "Follow a slowly gliding dot. This shows your shake while you move."),
          step(3, "Tap", "6 clicks", "Click a few bubbles. This shows whether clicks slip or double up."),
        ),
        h(
          "p",
          { class: "muted" },
          "Steadying pauses during the game so it can see your natural movement, and turns back on afterwards. There is no score and no way to get it wrong.",
        ),
        h(
          "div",
          { class: "btn-row" },
          h("button", { class: "btn primary big", onclick: () => void play() }, icon("play"), "Start calibration"),
          lastReport
            ? h("button", { class: "btn big", onclick: () => showResults(lastReport!) }, "See last results")
            : null,
        ),
      ),
      last
        ? h(
            "section",
            { class: "card" },
            h("h2", null, "Last calibration"),
            h(
              "dl",
              { class: "kv" },
              h("dt", null, "Date"),
              h("dd", null, formatDate(last.date_unix)),
              h("dt", null, "Shake rhythm"),
              h("dd", null, last.frequency_hz ? `${last.frequency_hz.toFixed(1)} Hz` : "no clear rhythm"),
              h("dt", null, "Typical swing"),
              h("dd", null, `±${last.amplitude_px.toFixed(1)} px (${severityText(last.severity)})`),
              h("dt", null, "Shake removed"),
              h("dd", null, `about ${Math.round(last.reduction_percent)}%`),
            ),
          )
        : null,
    );
  }

  async function play() {
    const s = store.current();
    await api.beginCalibration();
    const game = startGame(unitScale(s.os));
    const recording = await game.result;
    if (!recording) {
      game.close();
      await api.endCalibration();
      showIntro();
      return;
    }
    game.showWorking();
    try {
      const [report] = await Promise.all([
        api.analyzeCalibration(recording),
        new Promise((r) => setTimeout(r, 900)),
      ]);
      lastReport = report;
      showResults(report);
    } catch (e) {
      showIntro(String(e));
    } finally {
      game.close();
      await api.endCalibration();
    }
  }

  function showResults(r: CalibrationReport) {
    cleanup?.();
    const p = r.profile;
    const minimal = p.severity === "minimal";
    const replayCanvas = new ReplayCanvas();
    const followBtn = h("button", { "aria-pressed": "true", onclick: () => pick("follow") }, "Follow round");
    const restBtn = h("button", { "aria-pressed": "false", onclick: () => pick("rest") }, "Rest round");
    function pick(which: "follow" | "rest") {
      followBtn.setAttribute("aria-pressed", String(which === "follow"));
      restBtn.setAttribute("aria-pressed", String(which === "rest"));
      replayCanvas.play(which === "follow" ? r.follow_replay : r.hold_replay);
    }
    const spectrum = h("canvas");
    const applyBtn = h(
      "button",
      {
        class: "btn primary big",
        onclick: async () => {
          store.set(await api.applyCalibration(r));
          applyBtn.replaceChildren(icon("check"), "Settings applied");
          applyBtn.disabled = true;
          toast("Your personal settings are on.");
        },
      },
      icon("check"),
      "Use these settings",
    );

    el.replaceChildren(
      h(
        "header",
        { class: "view-head" },
        h("div", null, h("h1", null, "Your results"), h("p", null, "Measured from your own movement just now.")),
      ),
      h(
        "section",
        { class: "card" },
        h(
          "div",
          { class: "result-hero" },
          h("div", { class: "big-number" }, `${Math.round(r.reduction_percent)}%`),
          h(
            "div",
            null,
            h(
              "h2",
              null,
              minimal
                ? "Your pointer is already quite steady"
                : `Calmcursor can remove about ${Math.round(r.reduction_percent)}% of your shake`,
            ),
            h(
              "p",
              { class: "muted" },
              `We found ${severityText(p.severity)}${p.frequency_hz ? ` with a rhythm of about ${p.frequency_hz.toFixed(1)} times a second` : ""}. ` +
                `Smoothing adds about ${Math.round(r.lag_ms)} ms of delay to deliberate moves.`,
            ),
          ),
        ),
      ),
      h(
        "section",
        { class: "stats" },
        stat(p.frequency_hz ? `${p.frequency_hz.toFixed(1)} Hz` : "–", "shake rhythm"),
        stat(`±${p.amplitude_px.toFixed(1)} px`, "typical swing"),
        stat(`${r.taps.median_drift_px.toFixed(1)} px`, "slip while clicking"),
        stat(`${Math.round(r.moving_reduction_percent)}%`, "shake removed while moving"),
      ),
      h(
        "div",
        { class: "result-grid" },
        h(
          "section",
          { class: "card" },
          h(
            "div",
            { class: "card-head" },
            h("h2", null, "Your path, replayed"),
            h("div", { class: "segmented", role: "group", "aria-label": "Round" }, followBtn, restBtn),
          ),
          h("div", { class: "replay-wrap" }, replayCanvas.canvas),
          h(
            "div",
            { class: "legend" },
            h("span", null, h("i", { class: "raw" }), "Your hand (raw)"),
            h("span", null, h("i", { class: "smooth" }), "With these settings"),
          ),
        ),
        h(
          "section",
          { class: "card" },
          h("h2", null, "Recommended settings"),
          h(
            "dl",
            { class: "kv" },
            h("dt", null, "Steadiness"),
            h("dd", null, `${Math.round(r.steadiness)} · ${steadinessLabel(r.steadiness)}`),
            h("dt", null, "Filter"),
            h("dd", { class: "mono" }, `${r.filter.min_cutoff.toFixed(2)} Hz · β ${r.filter.beta.toFixed(4)}`),
            h("dt", null, "Click lock"),
            h("dd", null, `hold up to ${r.click.anchor_ms} ms, drag after ${Math.round(r.click.anchor_radius_px)} px`),
            h("dt", null, "Double-press filter"),
            h("dd", null, r.click.debounce_ms > 0 ? `${r.click.debounce_ms} ms` : "off (no bounces seen)"),
          ),
          h("div", { class: "spectrum", "aria-hidden": "true" }, spectrum),
          h("p", { class: "small muted" }, "Strength of fast movement at each rhythm (2–15 Hz)."),
          h(
            "div",
            { class: "btn-row" },
            applyBtn,
            h("button", { class: "btn big", onclick: () => void play() }, icon("refresh"), "Try again"),
          ),
          h("button", { class: "btn ghost", onclick: () => go("home") }, "Back to Home"),
        ),
      ),
    );
    requestAnimationFrame(() => {
      pick("follow");
      drawSpectrum(spectrum, p.spectrum, p.frequency_hz);
    });
    cleanup = () => replayCanvas.stop();
  }

  showIntro();
  return {
    el,
    update(_s: Snapshot) {},
    destroy() {
      cleanup?.();
    },
  };
}

function step(n: number, title: string, secs: string, text: string) {
  return h(
    "div",
    { class: "step" },
    h("div", { class: "num" }, String(n)),
    h("h3", null, title),
    h("div", { class: "secs" }, secs),
    h("p", { class: "muted small" }, text),
  );
}

function stat(value: string, label: string) {
  return h("div", { class: "stat" }, h("div", { class: "value" }, value), h("div", { class: "label" }, label));
}

/** Plays back a recorded path: raw and smoothed, drawn as they happened. */
class ReplayCanvas {
  readonly canvas = h("canvas");
  private raf = 0;
  private replay: Replay | null = null;
  private start = 0;

  play(r: Replay) {
    this.replay = r;
    this.start = performance.now();
    cancelAnimationFrame(this.raf);
    const loop = () => {
      this.draw();
      this.raf = requestAnimationFrame(loop);
    };
    this.raf = requestAnimationFrame(loop);
  }

  stop() {
    cancelAnimationFrame(this.raf);
  }

  private draw() {
    const r = this.replay;
    const ctx = this.canvas.getContext("2d");
    if (!r || !ctx || r.raw.length < 2) return;
    const { w, h: hh, dpr } = fitCanvas(this.canvas);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, hh);

    const pts = [...r.raw, ...r.target];
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const [x, y] of pts) {
      minX = Math.min(minX, x);
      maxX = Math.max(maxX, x);
      minY = Math.min(minY, y);
      maxY = Math.max(maxY, y);
    }
    const pad = 28;
    const spanX = Math.max(maxX - minX, 40);
    const spanY = Math.max(maxY - minY, 40);
    const scale = Math.min((w - 2 * pad) / spanX, (hh - 2 * pad) / spanY);
    const cx = (minX + maxX) / 2;
    const cy = (minY + maxY) / 2;
    const X = (x: number) => w / 2 + (x - cx) * scale;
    const Y = (y: number) => hh / 2 + (y - cy) * scale;

    // 60 points per second of recording; play at real speed, then hold.
    const total = r.raw.length;
    const elapsed = (performance.now() - this.start) / 1000;
    const loopLen = total / 60 + 1.5;
    const n = prefersReducedMotion()
      ? total
      : Math.min(total, Math.max(2, Math.floor((elapsed % loopLen) * 60)));

    const accent = cssVar("--accent") || "#1b8474";
    const raw = cssVar("--raw") || "#e4572e";
    const line = cssVar("--line") || "#dfe7e5";
    const ink = cssVar("--ink") || "#10211e";

    const stroke = (p: [number, number][], count: number, color: string, width: number, alpha = 1, dash: number[] = []) => {
      ctx.setLineDash(dash);
      ctx.globalAlpha = alpha;
      ctx.strokeStyle = color;
      ctx.lineWidth = width;
      ctx.lineCap = "round";
      ctx.lineJoin = "round";
      ctx.beginPath();
      for (let i = 0; i < Math.min(count, p.length); i++) {
        if (i === 0) ctx.moveTo(X(p[i][0]), Y(p[i][1]));
        else ctx.lineTo(X(p[i][0]), Y(p[i][1]));
      }
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.globalAlpha = 1;
    };
    if (r.target.length) stroke(r.target, r.target.length, line, 3, 1, [6, 9]);
    stroke(r.raw, n, raw, 1.7, 0.9);
    stroke(r.smoothed, n, accent, 4);
    const last = r.smoothed[Math.min(n, r.smoothed.length) - 1];
    if (last) drawCursor(ctx, X(last[0]), Y(last[1]), ink);
  }
}

function drawSpectrum(canvas: HTMLCanvasElement, spectrum: [number, number][], peak: number | null) {
  const ctx = canvas.getContext("2d");
  if (!ctx || spectrum.length === 0) return;
  const { w, h: hh, dpr } = fitCanvas(canvas);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, hh);
  const accent = cssVar("--accent") || "#1b8474";
  const muted = cssVar("--muted") || "#566764";
  const line = cssVar("--line") || "#dfe7e5";
  const fMin = spectrum[0][0];
  const fMax = spectrum[spectrum.length - 1][0];
  const base = hh - 24;
  const X = (f: number) => 8 + ((f - fMin) / (fMax - fMin)) * (w - 16);
  const barW = Math.max(2, (w - 16) / spectrum.length - 2);
  for (const [f, p] of spectrum) {
    const near = peak !== null && Math.abs(f - peak) < 0.3;
    ctx.fillStyle = near ? accent : line;
    const bh = Math.max(2, p * (base - 10));
    ctx.fillRect(X(f) - barW / 2, base - bh, barW, bh);
  }
  ctx.fillStyle = muted;
  ctx.font = "13px " + (cssVar("--font") || "sans-serif");
  ctx.textAlign = "center";
  for (const f of [2, 4, 6, 8, 10, 12, 14]) ctx.fillText(`${f}`, X(f), hh - 6);
  if (peak !== null) {
    ctx.fillStyle = accent;
    ctx.font = "bold 14px " + (cssVar("--font") || "sans-serif");
    ctx.fillText(`${peak.toFixed(1)} Hz`, Math.min(w - 40, Math.max(40, X(peak))), 14);
  }
}
