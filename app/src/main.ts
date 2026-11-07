// Calmcursor window: navigation and screen switching.

import { onNavigate } from "./api";
import * as store from "./store";
import { h, icon } from "./ui";
import { aboutView } from "./views/about";
import { calibrateView } from "./views/calibrate";
import { homeView } from "./views/home";
import { settingsView } from "./views/settings";
import type { View } from "./views/shared";

type Route = "home" | "calibrate" | "settings" | "about";

const ROUTES: { id: Route; label: string; icon: string; make: (go: (r: string) => void) => View }[] = [
  { id: "home", label: "Home", icon: "home", make: (go) => homeView(go) },
  { id: "calibrate", label: "Calibrate", icon: "target", make: (go) => calibrateView(go) },
  { id: "settings", label: "Settings", icon: "sliders", make: () => settingsView() },
  { id: "about", label: "About", icon: "info", make: () => aboutView() },
];

const nav = document.getElementById("nav")!;
const viewEl = document.getElementById("view")!;
const foot = document.getElementById("sidebar-foot")!;

let active: { route: Route; view: View; unsubscribe: () => void } | null = null;

function go(route: string) {
  const r = ROUTES.find((x) => x.id === route) ?? ROUTES[0];
  if (active?.route === r.id) return;
  active?.unsubscribe();
  active?.view.destroy?.();
  const view = r.make(go);
  viewEl.replaceChildren(view.el);
  viewEl.scrollTop = 0;
  const unsubscribe = store.subscribe((s) => view.update(s));
  active = { route: r.id, view, unsubscribe };
  for (const b of nav.querySelectorAll("button")) {
    if (b.dataset.route === r.id) b.setAttribute("aria-current", "page");
    else b.removeAttribute("aria-current");
  }
  viewEl.focus({ preventScroll: true });
}

async function main() {
  for (const r of ROUTES) {
    nav.append(
      h("li", null, h("button", { "data-route": r.id, onclick: () => go(r.id) }, icon(r.icon), r.label)),
    );
  }
  await store.init();

  const pill = h("span", { class: "pill" });
  const version = h("div", { class: "version-line" });
  foot.append(pill, version);
  store.subscribe((s) => {
    const on = s.running && s.enabled && !s.calibrating;
    pill.className = `pill ${!s.running ? "warn" : on ? "on" : "off"}`;
    pill.replaceChildren(
      h("span", { class: "dot" }),
      s.calibrating ? "Calibrating" : !s.running ? "Not running" : s.enabled ? "Steadying on" : "Paused",
    );
    version.textContent = `Version ${s.build.version} · build ${s.build.build_number}`;
  });

  await onNavigate((route) => go(route));
  go(new URLSearchParams(location.search).get("view") ?? "home");
}

void main();
