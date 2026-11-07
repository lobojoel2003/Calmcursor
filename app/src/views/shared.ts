// Pieces used by several screens.

import { api, type Severity, type Snapshot } from "../api";
import * as store from "../store";
import { h, icon, toast } from "../ui";

export interface View {
  el: HTMLElement;
  update(s: Snapshot): void;
  destroy?(): void;
}

export function steadinessLabel(v: number): string {
  if (v < 25) return "Light";
  if (v < 60) return "Balanced";
  if (v < 85) return "Strong";
  return "Strongest";
}

export function severityText(s: Severity): string {
  switch (s) {
    case "minimal":
      return "very little shake";
    case "mild":
      return "a mild shake";
    case "moderate":
      return "a moderate shake";
    case "strong":
      return "a strong shake";
  }
}

/** Explains why steadying is not running and how to fix it. */
export function errorBanner(s: Snapshot): HTMLElement[] {
  const out: HTMLElement[] = [];
  const retry = h(
    "button",
    {
      class: "btn primary",
      onclick: async () => {
        const next = await api.retryEngine();
        store.set(next);
        toast(next.running ? "Steadying is running." : "Still not running. Check the steps above.");
      },
    },
    icon("refresh"),
    "Try again",
  );
  const e = s.error;
  if (!s.running && e) {
    let title = "Steadying isn't running";
    let message = "";
    let help = "";
    const buttons: HTMLElement[] = [];
    if (e.kind === "permission_denied") {
      title = "Calmcursor needs your permission";
      message = e.message.charAt(0).toUpperCase() + e.message.slice(1) + ".";
      help = e.help;
      if (s.os === "macos") {
        buttons.push(
          h("button", { class: "btn", onclick: () => void api.requestPermission() }, "Ask macOS again"),
        );
        if (s.permission_url) {
          const url = s.permission_url;
          buttons.push(
            h("button", { class: "btn", onclick: () => void api.openUrl(url) }, icon("external"), "Open System Settings"),
          );
        }
      } else if (s.permission_url) {
        const url = s.permission_url;
        buttons.push(
          h("button", { class: "btn", onclick: () => void api.openUrl(url) }, icon("external"), "Full instructions"),
        );
      }
    } else if (e.kind === "no_devices") {
      title = "No mouse or trackball found";
      help = e.help;
    } else if (e.kind === "unsupported") {
      message = "This operating system is not supported yet.";
    } else {
      message = e.message;
    }
    buttons.push(retry);
    out.push(
      h(
        "div",
        { class: "banner", role: "alert" },
        icon("alert"),
        h(
          "div",
          { class: "body" },
          h("h3", null, title),
          message ? h("p", null, message) : null,
          help ? h("pre", null, help) : null,
          h("div", { class: "btn-row" }, buttons),
          h("p", { class: "small muted" }, "Meanwhile, the live trace below plays a demo so you can see how steadying works."),
        ),
      ),
    );
  }
  if (s.running && s.warning) {
    out.push(h("div", { class: "banner info", role: "status" }, icon("info"), h("div", { class: "body" }, h("p", null, s.warning))));
  }
  if (s.shortcut_error) {
    out.push(
      h("div", { class: "banner info", role: "status" }, icon("info"), h("div", { class: "body" }, h("p", null, `Pause shortcut: ${s.shortcut_error}. You can choose another in Settings.`))),
    );
  }
  return out;
}

/** CSS pixels → engine units for calibration recordings. */
export function unitScale(os: string): number {
  // Windows steadies in physical pixels; macOS in points (= CSS pixels);
  // Linux in device counts, which are close to pixels.
  return os === "windows" ? window.devicePixelRatio || 1 : 1;
}

export function formatDate(unix: number): string {
  return new Date(unix * 1000).toLocaleDateString(undefined, {
    year: "numeric",
    month: "long",
    day: "numeric",
  });
}
