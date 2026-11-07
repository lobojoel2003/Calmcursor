// Live trace: the raw (shaky) pointer path next to the steadied one.

import { TraceFlags, type TraceTuple } from "./api";
import { cssVar, fitCanvas, prefersReducedMotion } from "./ui";

/** Seconds of history drawn. */
const WINDOW_S = 2.5;
/** Smallest area (engine units) the camera zooms in to, so tremor is visible. */
const MIN_SPAN = 70;
const T = 0, RX = 1, RY = 2, OX = 3, OY = 4, F = 5;

export class TraceView {
  readonly canvas = document.createElement("canvas");
  private samples: TraceTuple[] = [];
  private cam = { x: 0, y: 0, span: 200, ready: false };
  private raf = 0;
  private running = false;
  private frameCount = 0;
  private metric: number | null = null;
  /** Called with the share of shake removed (0–100), or null if unknown. */
  onMetric?: (percent: number | null) => void;

  get hasData(): boolean {
    return this.samples.length > 1;
  }

  push(batch: TraceTuple[]) {
    if (batch.length === 0) return;
    const last = this.samples[this.samples.length - 1];
    // A new source (or restart) begins a new clock.
    if (last && batch[0][T] < last[T] - 0.25) this.clear();
    this.samples.push(...batch);
    const cutoff = batch[batch.length - 1][T] - WINDOW_S - 0.5;
    let i = 0;
    while (i < this.samples.length && this.samples[i][T] < cutoff) i++;
    if (i > 0) this.samples.splice(0, i);
  }

  clear() {
    this.samples = [];
    this.cam.ready = false;
    this.metric = null;
    this.onMetric?.(null);
  }

  start() {
    if (this.running) return;
    this.running = true;
    const loop = () => {
      if (!this.running) return;
      this.draw();
      this.raf = requestAnimationFrame(loop);
    };
    this.raf = requestAnimationFrame(loop);
  }

  stop() {
    this.running = false;
    cancelAnimationFrame(this.raf);
  }

  private draw() {
    const ctx = this.canvas.getContext("2d");
    if (!ctx) return;
    const { w, h, dpr } = fitCanvas(this.canvas);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    if (this.samples.length < 2 || w === 0) return;

    const now = this.samples[this.samples.length - 1][T];
    const vis = this.samples.filter((s) => s[T] >= now - WINDOW_S);
    if (vis.length < 2) return;

    // Camera: frame both paths, never closer than MIN_SPAN.
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const s of vis) {
      minX = Math.min(minX, s[RX], s[OX]);
      maxX = Math.max(maxX, s[RX], s[OX]);
      minY = Math.min(minY, s[RY], s[OY]);
      maxY = Math.max(maxY, s[RY], s[OY]);
    }
    const aspect = w / h;
    const spanX = Math.max(maxX - minX, MIN_SPAN * aspect);
    const spanY = Math.max(maxY - minY, MIN_SPAN);
    const span = Math.max(spanX / aspect, spanY) * 1.35;
    const cx = (minX + maxX) / 2;
    const cy = (minY + maxY) / 2;
    if (!this.cam.ready || prefersReducedMotion()) {
      this.cam = { x: cx, y: cy, span, ready: true };
    } else {
      const k = 0.08;
      this.cam.x += (cx - this.cam.x) * k;
      this.cam.y += (cy - this.cam.y) * k;
      this.cam.span += (span - this.cam.span) * k;
    }
    const scale = h / this.cam.span;
    const px = (x: number) => w / 2 + (x - this.cam.x) * scale;
    const py = (y: number) => h / 2 + (y - this.cam.y) * scale;

    const raw = cssVar("--raw") || "#e4572e";
    const accent = cssVar("--accent") || "#1b8474";
    const ink = cssVar("--ink") || "#10211e";

    this.path(ctx, vis, now, RX, RY, px, py, raw, 1.8, 0.9);
    this.path(ctx, vis, now, OX, OY, px, py, accent, 4.5, 1);

    // Click rings, fading with age.
    for (const s of vis) {
      if ((s[F] & TraceFlags.PRESS) === 0) continue;
      const age = now - s[T];
      ctx.globalAlpha = Math.max(0, 1 - age / WINDOW_S);
      ctx.lineWidth = 3;
      ctx.strokeStyle = accent;
      ctx.fillStyle = cssVar("--surface") || "#fff";
      ctx.beginPath();
      ctx.arc(px(s[OX]), py(s[OY]), 11 + Math.min(age, 0.4) * 20, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();
    }
    ctx.globalAlpha = 1;

    // Where the hand is (raw) and the pointer people see.
    const last = vis[vis.length - 1];
    ctx.fillStyle = raw;
    ctx.beginPath();
    ctx.arc(px(last[RX]), py(last[RY]), 4, 0, Math.PI * 2);
    ctx.fill();
    drawCursor(ctx, px(last[OX]), py(last[OY]), ink);

    if (++this.frameCount % 8 === 0) this.updateMetric(vis);
  }

  private path(
    ctx: CanvasRenderingContext2D,
    vis: TraceTuple[],
    now: number,
    ix: number,
    iy: number,
    px: (x: number) => number,
    py: (y: number) => number,
    color: string,
    width: number,
    maxAlpha: number,
  ) {
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.lineWidth = width;
    ctx.strokeStyle = color;
    const chunk = Math.max(4, Math.ceil(vis.length / 24));
    for (let start = 0; start < vis.length - 1; start += chunk) {
      const end = Math.min(vis.length - 1, start + chunk);
      const age = now - vis[end][T];
      ctx.globalAlpha = maxAlpha * Math.max(0.08, 1 - age / WINDOW_S);
      ctx.beginPath();
      ctx.moveTo(px(vis[start][ix]), py(vis[start][iy]));
      for (let i = start + 1; i <= end; i++) ctx.lineTo(px(vis[i][ix]), py(vis[i][iy]));
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
  }

  /** Share of fast (tremor) movement removed, from the last couple of seconds. */
  private updateMetric(vis: TraceTuple[]) {
    if (vis.length < 40 || vis[vis.length - 1][T] - vis[0][T] < 1.2) return;
    const residual = (ix: number, iy: number) => {
      const half = 0.1;
      let sum = 0;
      let n = 0;
      let lo = 0;
      let hi = 0;
      let accX = 0;
      let accY = 0;
      for (let i = 0; i < vis.length; i++) {
        const t = vis[i][T];
        while (hi < vis.length && vis[hi][T] <= t + half) {
          accX += vis[hi][ix];
          accY += vis[hi][iy];
          hi++;
        }
        while (vis[lo][T] < t - half) {
          accX -= vis[lo][ix];
          accY -= vis[lo][iy];
          lo++;
        }
        const count = hi - lo;
        const dx = vis[i][ix] - accX / count;
        const dy = vis[i][iy] - accY / count;
        sum += dx * dx + dy * dy;
        n++;
      }
      return Math.sqrt(sum / n);
    };
    const r = residual(RX, RY);
    const o = residual(OX, OY);
    const value = r > 0.35 ? Math.max(0, Math.min(100, (1 - o / r) * 100)) : null;
    this.metric =
      value === null ? null : this.metric === null ? value : this.metric * 0.7 + value * 0.3;
    this.onMetric?.(this.metric);
  }
}

/** A classic arrow pointer with its tip at (x, y). */
export function drawCursor(ctx: CanvasRenderingContext2D, x: number, y: number, ink: string) {
  const s = 1.25;
  ctx.save();
  ctx.translate(x, y);
  ctx.scale(s, s);
  ctx.beginPath();
  ctx.moveTo(0, 0);
  ctx.lineTo(0, 17);
  ctx.lineTo(4.2, 13.2);
  ctx.lineTo(7, 19.6);
  ctx.lineTo(9.8, 18.4);
  ctx.lineTo(7.1, 12.2);
  ctx.lineTo(12.4, 12.2);
  ctx.closePath();
  ctx.fillStyle = "#ffffff";
  ctx.strokeStyle = ink;
  ctx.lineWidth = 1.4;
  ctx.lineJoin = "round";
  ctx.fill();
  ctx.stroke();
  ctx.restore();
}
