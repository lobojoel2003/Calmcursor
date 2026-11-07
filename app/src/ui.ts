// Tiny DOM helpers: no framework needed for a handful of screens.

type Child = Node | string | number | null | undefined | false;
type Attrs = Record<string, unknown>;

export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs?: Attrs | null,
  ...children: (Child | Child[])[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  if (attrs) {
    for (const [key, value] of Object.entries(attrs)) {
      if (value === undefined || value === null || value === false) continue;
      if (key.startsWith("on") && typeof value === "function") {
        el.addEventListener(key.slice(2).toLowerCase(), value as EventListener);
      } else if (key === "class") {
        el.className = String(value);
      } else if (key === "style" && typeof value === "object") {
        Object.assign(el.style, value);
      } else if (key === "html") {
        el.innerHTML = String(value);
      } else if (key in el && typeof value !== "string") {
        (el as unknown as Record<string, unknown>)[key] = value;
      } else {
        el.setAttribute(key, value === true ? "" : String(value));
      }
    }
  }
  append(el, children);
  return el;
}

/** Replaces an element's children, skipping null/false entries. */
export function replace(el: Element, ...children: (Child | Child[])[]) {
  el.replaceChildren();
  append(el, children);
}

function append(el: Element, children: (Child | Child[])[]) {
  for (const c of children) {
    if (Array.isArray(c)) append(el, c);
    else if (c === null || c === undefined || c === false) continue;
    else el.append(c instanceof Node ? c : String(c));
  }
}

const ICONS: Record<string, string> = {
  home: '<path d="M3 10.5 12 3l9 7.5V20a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z"/>',
  target:
    '<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="5"/><circle cx="12" cy="12" r="1.5" fill="currentColor"/>',
  sliders:
    '<path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0"/><circle cx="16" cy="6" r="2"/><circle cx="10" cy="12" r="2"/><circle cx="18" cy="18" r="2"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5v.5"/>',
  alert: '<path d="M12 3 2 20h20z"/><path d="M12 10v4M12 17v.5"/>',
  play: '<path d="M8 5v14l11-7z" fill="currentColor"/>',
  refresh: '<path d="M20 12a8 8 0 1 1-2.3-5.6M20 4v5h-5"/>',
  external: '<path d="M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"/>',
  check: '<path d="m5 12 5 5 9-10"/>',
  github:
    '<path d="M9 19c-4 1.5-4-2-6-2.5m12 5v-3.5c0-1 .1-1.4-.5-2 2.8-.3 5.5-1.4 5.5-6a4.6 4.6 0 0 0-1.3-3.2 4.2 4.2 0 0 0-.1-3.2s-1.1-.3-3.5 1.3a12.3 12.3 0 0 0-6.2 0C6.5 2.8 5.4 3.1 5.4 3.1a4.2 4.2 0 0 0-.1 3.2A4.6 4.6 0 0 0 4 9.5c0 4.6 2.7 5.7 5.5 6-.6.6-.6 1.2-.5 2V21"/>',
  close: '<path d="M6 6l12 12M18 6 6 18"/>',
};

export function icon(name: keyof typeof ICONS | string): SVGSVGElement {
  const t = document.createElement("template");
  t.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICONS[name] ?? ""}</svg>`;
  return t.content.firstElementChild as SVGSVGElement;
}

/** An accessible on/off switch. */
export function switchButton(
  checked: boolean,
  label: string,
  onChange: (on: boolean) => void,
): HTMLButtonElement {
  const b = h("button", {
    class: "switch",
    role: "switch",
    "aria-checked": String(checked),
    "aria-label": label,
    onclick: () => {
      const on = b.getAttribute("aria-checked") !== "true";
      b.setAttribute("aria-checked", String(on));
      onChange(on);
    },
  });
  return b;
}

export function toggleRow(
  title: string,
  hint: string,
  checked: boolean,
  onChange: (on: boolean) => void,
): { el: HTMLElement; set: (on: boolean) => void } {
  const sw = switchButton(checked, title, onChange);
  const el = h(
    "div",
    { class: "toggle-row" },
    h("div", { class: "text" }, h("span", null, title), h("span", null, hint)),
    sw,
  );
  return { el, set: (on) => sw.setAttribute("aria-checked", String(on)) };
}

export interface SliderOptions {
  label: string;
  min: number;
  max: number;
  step: number;
  value: number;
  format: (v: number) => string;
  hint?: string;
  scale?: [string, string];
  onInput?: (v: number) => void;
  onChange: (v: number) => void;
}

let sliderId = 0;

/** A labelled range slider with a large thumb and a live value. */
export function slider(o: SliderOptions): { el: HTMLElement; set: (v: number) => void } {
  const id = `slider-${++sliderId}`;
  const out = h("output", { for: id }, o.format(o.value));
  const input = h("input", {
    id,
    type: "range",
    min: String(o.min),
    max: String(o.max),
    step: String(o.step),
    value: String(o.value),
  });
  const paint = () => {
    const v = Number(input.value);
    input.style.setProperty("--fill", `${((v - o.min) / (o.max - o.min)) * 100}%`);
    out.textContent = o.format(v);
    input.setAttribute("aria-valuetext", o.format(v));
  };
  input.addEventListener("input", () => {
    paint();
    o.onInput?.(Number(input.value));
  });
  input.addEventListener("change", () => o.onChange(Number(input.value)));
  paint();
  const el = h(
    "div",
    { class: "field" },
    h("div", { class: "field-head" }, h("label", { for: id }, o.label), out),
    input,
    o.scale ? h("div", { class: "field-scale" }, h("span", null, o.scale[0]), h("span", null, o.scale[1])) : null,
    o.hint ? h("p", { class: "field-hint" }, o.hint) : null,
  );
  return {
    el,
    set: (v) => {
      if (document.activeElement === input) return;
      input.value = String(v);
      paint();
    },
  };
}

let toastTimer: number | undefined;

export function toast(message: string) {
  document.querySelector(".toast")?.remove();
  const t = h("div", { class: "toast", role: "status" }, message);
  document.body.append(t);
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => t.remove(), 3200);
}

/** Calls `fn` at most once per `ms`, always with the latest arguments. */
export function debounce<A extends unknown[]>(fn: (...a: A) => void, ms: number) {
  let timer: number | undefined;
  return (...a: A) => {
    window.clearTimeout(timer);
    timer = window.setTimeout(() => fn(...a), ms);
  };
}

export function formatShortcut(s: string, os: string): string {
  if (!s) return "none";
  const mac = os === "macos";
  return s
    .split("+")
    .map((k) => {
      switch (k.toLowerCase()) {
        case "commandorcontrol":
        case "cmdorctrl":
          return mac ? "⌘" : "Ctrl";
        case "alt":
        case "option":
          return mac ? "⌥" : "Alt";
        case "shift":
          return mac ? "⇧" : "Shift";
        case "control":
        case "ctrl":
          return mac ? "⌃" : "Ctrl";
        default:
          return k;
      }
    })
    .join(mac ? "" : "+");
}

export function prefersReducedMotion(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** Sizes a canvas for crisp drawing; returns the CSS size. */
export function fitCanvas(canvas: HTMLCanvasElement): { w: number; h: number; dpr: number } {
  const dpr = window.devicePixelRatio || 1;
  const w = canvas.clientWidth;
  const hh = canvas.clientHeight;
  if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(hh * dpr)) {
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(hh * dpr);
  }
  return { w, h: hh, dpr };
}

export function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}
