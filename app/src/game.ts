// The 30-second calibration game: Rest, Follow, Tap.
//
// Steadying is paused while it runs, so what it records is the person's
// natural movement. Nothing is scored; there is no way to "fail".

import type { CalibrationRecording, PointerSample } from "./api";
import { cssVar, fitCanvas, h, icon, prefersReducedMotion } from "./ui";

type Phase = "countdown" | "rest" | "follow" | "tap" | "done";

const REST_S = 10;
const REST_MAX_S = 22;
const FOLLOW_S = 12;
const FOLLOW_PERIOD_S = 6.5;
/** How long the pointer must rest on the dot before it starts gliding. */
const FOLLOW_READY_S = 0.5;
const TAP_COUNT = 6;
const TAP_MAX_S = 30;
const TAP_RADIUS = 38;
/** Fill gaps when the pointer is perfectly still (no events fire). */
const FILL_GAP_S = 0.05;

const ROUNDS: { id: Phase; label: string; title: string; text: string }[] = [
  {
    id: "rest",
    label: "1  Rest",
    title: "Rest the pointer in the circle",
    text: "Let your hand relax. Small wobbles are fine — that is what we measure.",
  },
  {
    id: "follow",
    label: "2  Follow",
    title: "Follow the dot",
    text: "Put the pointer on the dot to start, then keep it close as it glides.",
  },
  {
    id: "tap",
    label: "3  Tap",
    title: "Click each bubble",
    text: "Take your time. Click once on each bubble as it appears.",
  },
];

export interface GameHandle {
  /** The recording, or null if the person left the game. */
  result: Promise<CalibrationRecording | null>;
  /** Shows a message while the results are worked out. */
  showWorking(): void;
  close(): void;
}

/**
 * Starts the game in a full-window overlay. `unitScale` converts CSS pixels
 * into the units the steadying engine works in.
 */
export function startGame(unitScale: number): GameHandle {
  let resolve!: (r: CalibrationRecording | null) => void;
  const result = new Promise<CalibrationRecording | null>((r) => (resolve = r));
  const game = new Game(unitScale, resolve);
  return { result, showWorking: () => game.showWorking(), close: () => game.close() };
}

class Game {
  private root: HTMLDivElement;
  private canvas: HTMLCanvasElement;
  private pills: HTMLSpanElement[];
  private titleEl: HTMLHeadingElement;
  private textEl: HTMLParagraphElement;
  private bar: HTMLDivElement;
  private center: HTMLDivElement;
  private phase: Phase = "countdown";
  private phaseStart = 0;
  private lastFrame = 0;
  private raf = 0;
  private pointer: { x: number; y: number } | null = null;
  private trail: { x: number; y: number; t: number }[] = [];
  private rec: CalibrationRecording = { hold: [], follow: [], taps: [], presses: [] };
  private lastSampleT = 0;
  private restInside = 0;
  private restEntered = false;
  private followT0: number | null = null;
  private followReady = 0;
  private target: { x: number; y: number; born: number } | null = null;
  private pops: { x: number; y: number; t: number }[] = [];
  private pendingDown: { t: number; x: number; y: number } | null = null;
  private taps = 0;
  private finished = false;

  constructor(
    private unitScale: number,
    private resolve: (r: CalibrationRecording | null) => void,
  ) {
    this.canvas = h("canvas");
    this.pills = ROUNDS.map((r) => h("span", null, r.label));
    this.titleEl = h("h2");
    this.textEl = h("p");
    this.bar = h("div");
    this.center = h("div", { class: "game-center" });
    const exit = h(
      "button",
      { class: "btn", onclick: () => this.cancel(), "aria-label": "Leave the calibration" },
      icon("close"),
      "Leave",
    );
    this.root = h(
      "div",
      { class: "game", role: "application", "aria-label": "Calibration game" },
      this.canvas,
      h("div", { class: "game-hud" }, h("div", { class: "game-steps" }, this.pills), exit),
      h("div", { class: "game-title", "aria-live": "polite" }, this.titleEl, this.textEl),
      h("div", { class: "game-progress" }, this.bar),
      this.center,
    );
    document.body.append(this.root);
    this.root.addEventListener("pointermove", this.onMove);
    this.root.addEventListener("pointerdown", this.onDown);
    this.root.addEventListener("pointerup", this.onUp);
    this.root.addEventListener("contextmenu", (e) => e.preventDefault());
    window.addEventListener("keydown", this.onKey);
    this.countdown();
    this.raf = requestAnimationFrame(this.frame);
  }

  private get w() {
    return this.root.clientWidth;
  }

  private get h() {
    return this.root.clientHeight;
  }

  private now() {
    return performance.now() / 1000;
  }

  private async countdown() {
    this.setRound(-1, "Get comfortable", "Hold the mouse the way you usually do.");
    for (const n of [3, 2, 1]) {
      if (this.finished) return;
      this.center.replaceChildren(h("div", { class: "count", "aria-live": "assertive" }, String(n)));
      await new Promise((r) => setTimeout(r, 800));
    }
    this.center.replaceChildren();
    this.enter("rest");
  }

  private setRound(index: number, title: string, text: string) {
    this.pills.forEach((p, i) => {
      p.className = i === index ? "active" : i < index ? "done" : "";
    });
    this.titleEl.textContent = title;
    this.textEl.textContent = text;
  }

  private enter(phase: Phase) {
    this.phase = phase;
    this.phaseStart = this.now();
    this.trail = [];
    this.lastSampleT = 0;
    const i = ROUNDS.findIndex((r) => r.id === phase);
    if (i >= 0) this.setRound(i, ROUNDS[i].title, ROUNDS[i].text);
    if (phase === "tap") this.spawnTarget();
    if (phase === "done") this.finish();
  }

  // ---- geometry ----

  private restCircle() {
    return { x: this.w / 2, y: this.h / 2 + 40, r: 72 };
  }

  private dotAt(t: number) {
    const a = Math.min(this.w * 0.32, 440);
    const b = Math.min(this.h * 0.2, 190);
    // Starts at the left end of the figure eight, away from the rest circle.
    const theta = -Math.PI / 2 + (2 * Math.PI * t) / FOLLOW_PERIOD_S;
    return { x: this.w / 2 + a * Math.sin(theta), y: this.h / 2 + 50 + b * Math.sin(2 * theta) };
  }

  private spawnTarget() {
    const margin = 120;
    const top = 210;
    let x = 0;
    let y = 0;
    for (let tries = 0; tries < 40; tries++) {
      x = margin + Math.random() * (this.w - 2 * margin);
      y = top + Math.random() * (this.h - top - margin);
      const prev = this.target ?? this.pointer;
      if (!prev || Math.hypot(x - prev.x, y - prev.y) > 220) break;
    }
    this.target = { x, y, born: this.now() };
  }

  // ---- input ----

  private sample(t: number, x: number, y: number) {
    const s = this.unitScale;
    if (this.phase === "rest" && this.restEntered) {
      this.rec.hold.push({ t, x: x * s, y: y * s });
      this.lastSampleT = t;
    } else if (this.phase === "follow" && this.followT0 !== null) {
      const d = this.dotAt(t - this.followT0);
      this.rec.follow.push({ t, x: x * s, y: y * s, tx: d.x * s, ty: d.y * s });
      this.lastSampleT = t;
    }
  }

  private onMove = (e: PointerEvent) => {
    const events =
      typeof e.getCoalescedEvents === "function" && e.getCoalescedEvents().length > 0
        ? e.getCoalescedEvents()
        : [e];
    for (const ev of events) {
      const t = ev.timeStamp / 1000;
      this.pointer = { x: ev.clientX, y: ev.clientY };
      this.trail.push({ x: ev.clientX, y: ev.clientY, t });
      this.sample(t, ev.clientX, ev.clientY);
    }
  };

  private onDown = (e: PointerEvent) => {
    if (e.button !== 0) return;
    const t = e.timeStamp / 1000;
    if (this.phase !== "tap") return;
    this.rec.presses.push({ t, down: true });
    const tg = this.target;
    if (tg && !this.pendingDown && Math.hypot(e.clientX - tg.x, e.clientY - tg.y) <= TAP_RADIUS * 1.7) {
      this.pendingDown = { t, x: e.clientX, y: e.clientY };
    }
  };

  private onUp = (e: PointerEvent) => {
    if (e.button !== 0 || this.phase !== "tap") return;
    const t = e.timeStamp / 1000;
    this.rec.presses.push({ t, down: false });
    const down = this.pendingDown;
    if (!down || !this.target) return;
    const s = this.unitScale;
    this.rec.taps.push({
      down_t: down.t,
      down_x: down.x * s,
      down_y: down.y * s,
      up_t: t,
      up_x: e.clientX * s,
      up_y: e.clientY * s,
    });
    this.pendingDown = null;
    this.pops.push({ x: this.target.x, y: this.target.y, t: this.now() });
    this.target = null;
    this.taps++;
    if (this.taps >= TAP_COUNT) {
      setTimeout(() => this.enter("done"), 450);
    } else {
      setTimeout(() => this.phase === "tap" && this.spawnTarget(), 300);
    }
  };

  private onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") this.cancel();
  };

  // ---- loop ----

  private frame = () => {
    if (this.finished && this.phase !== "done") return;
    const t = this.now();
    const dt = this.lastFrame ? Math.min(0.1, t - this.lastFrame) : 0;
    this.lastFrame = t;
    try {
      this.update(t, dt);
      this.draw(t);
    } catch (e) {
      console.error("calibration frame failed", e);
    }
    this.raf = requestAnimationFrame(this.frame);
  };

  private update(t: number, dt: number) {
    const elapsed = t - this.phaseStart;
    const p = this.pointer;
    // Still hands send no events; record the resting position so the
    // recording reflects the stillness.
    if (p && this.lastSampleT && t - this.lastSampleT > FILL_GAP_S) {
      this.sample(t, p.x, p.y);
    }
    if (this.phase === "rest") {
      const c = this.restCircle();
      const inside = p !== null && Math.hypot(p.x - c.x, p.y - c.y) <= c.r * 1.5;
      if (inside && !this.restEntered) {
        this.restEntered = true;
        if (p) this.sample(t, p.x, p.y);
      }
      if (inside) this.restInside += dt;
      this.progress(this.restInside / REST_S);
      if (this.restInside >= REST_S || (elapsed >= REST_MAX_S && this.rec.hold.length > 0)) {
        this.enter("follow");
      }
    } else if (this.phase === "follow") {
      if (this.followT0 === null) {
        const d = this.dotAt(0);
        const onDot = p !== null && Math.hypot(p.x - d.x, p.y - d.y) < 50;
        this.followReady = onDot ? this.followReady + dt : 0;
        if (p && this.followReady >= FOLLOW_READY_S) {
          this.followT0 = t;
          this.sample(t, p.x, p.y);
        }
        this.progress(0);
      } else {
        const ft = t - this.followT0;
        this.progress(ft / FOLLOW_S);
        if (ft >= FOLLOW_S) this.enter("tap");
      }
    } else if (this.phase === "tap") {
      this.progress(this.taps / TAP_COUNT);
      if (elapsed > TAP_MAX_S) this.enter("done");
    }
    const cutoff = t - 1.4;
    while (this.trail.length && this.trail[0].t < cutoff) this.trail.shift();
  }

  private progress(f: number) {
    this.bar.style.width = `${Math.max(0, Math.min(1, f)) * 100}%`;
  }

  private draw(t: number) {
    const ctx = this.canvas.getContext("2d");
    if (!ctx) return;
    const { w, h, dpr } = fitCanvas(this.canvas);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    const accent = cssVar("--accent") || "#1b8474";
    const soft = cssVar("--accent-soft") || "#e2f4f0";
    const raw = cssVar("--raw") || "#e4572e";
    const line = cssVar("--line") || "#dfe7e5";
    const calm = prefersReducedMotion();

    if (this.phase === "rest") {
      const c = this.restCircle();
      const breathe = calm ? 1 : 1 + 0.035 * Math.sin(t * Math.PI * 0.4);
      ctx.fillStyle = soft;
      ctx.strokeStyle = accent;
      ctx.lineWidth = 4;
      ctx.beginPath();
      ctx.arc(c.x, c.y, c.r * breathe, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();
      ctx.lineWidth = 10;
      ctx.lineCap = "round";
      ctx.beginPath();
      const f = Math.min(1, this.restInside / REST_S);
      ctx.arc(c.x, c.y, c.r + 20, -Math.PI / 2, -Math.PI / 2 + f * Math.PI * 2);
      ctx.stroke();
    } else if (this.phase === "follow") {
      ctx.setLineDash([6, 10]);
      ctx.strokeStyle = line;
      ctx.lineWidth = 3;
      ctx.beginPath();
      for (let i = 0; i <= 200; i++) {
        const d = this.dotAt((i / 200) * FOLLOW_PERIOD_S);
        if (i === 0) ctx.moveTo(d.x, d.y);
        else ctx.lineTo(d.x, d.y);
      }
      ctx.stroke();
      ctx.setLineDash([]);
      const d = this.dotAt(this.followT0 === null ? 0 : t - this.followT0);
      ctx.fillStyle = soft;
      ctx.beginPath();
      ctx.arc(d.x, d.y, 34, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = accent;
      ctx.beginPath();
      ctx.arc(d.x, d.y, 16, 0, Math.PI * 2);
      ctx.fill();
    } else if (this.phase === "tap") {
      const tg = this.target;
      if (tg) {
        const grow = calm ? 1 : Math.max(0.05, Math.min(1, (t - tg.born) / 0.25));
        ctx.fillStyle = soft;
        ctx.beginPath();
        ctx.arc(tg.x, tg.y, (TAP_RADIUS + 14) * grow, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillStyle = accent;
        ctx.beginPath();
        ctx.arc(tg.x, tg.y, TAP_RADIUS * grow, 0, Math.PI * 2);
        ctx.fill();
        ctx.strokeStyle = "#ffffff";
        ctx.lineWidth = 4;
        ctx.beginPath();
        ctx.arc(tg.x, tg.y, TAP_RADIUS * 0.45 * grow, 0, Math.PI * 2);
        ctx.stroke();
      }
      for (const p of this.pops) {
        const age = t - p.t;
        if (age < 0 || age > 0.5) continue;
        ctx.globalAlpha = 1 - age / 0.5;
        ctx.strokeStyle = accent;
        ctx.lineWidth = 4;
        ctx.beginPath();
        ctx.arc(p.x, p.y, TAP_RADIUS + age * 120, 0, Math.PI * 2);
        ctx.stroke();
        ctx.globalAlpha = 1;
      }
    }

    // The person's own (unsteadied) path.
    if (this.trail.length > 1) {
      ctx.strokeStyle = raw;
      ctx.lineWidth = 2.5;
      ctx.lineCap = "round";
      ctx.lineJoin = "round";
      for (let i = 1; i < this.trail.length; i++) {
        const a = this.trail[i - 1];
        const b = this.trail[i];
        ctx.globalAlpha = Math.max(0, 1 - (t - b.t) / 1.4) * 0.8;
        ctx.beginPath();
        ctx.moveTo(a.x, a.y);
        ctx.lineTo(b.x, b.y);
        ctx.stroke();
      }
      ctx.globalAlpha = 1;
    }
  }

  // ---- end ----

  private finish() {
    this.setRound(3, "Nicely done", "Working out your settings…");
    this.progress(1);
    const hold: PointerSample[] = this.rec.hold;
    this.resolve(hold.length > 0 ? this.rec : null);
  }

  showWorking() {
    this.titleEl.textContent = "Nicely done";
    this.textEl.textContent = "Working out your settings…";
  }

  private cancel() {
    if (this.finished) return;
    this.resolve(null);
    this.close();
  }

  close() {
    this.finished = true;
    cancelAnimationFrame(this.raf);
    window.removeEventListener("keydown", this.onKey);
    this.root.remove();
  }
}
