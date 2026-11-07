// Settings: everything adjustable, grouped and explained in plain words.

import { api, type Snapshot } from "../api";
import * as store from "../store";
import { debounce, formatShortcut, h, slider, toast, toggleRow } from "../ui";
import { steadinessLabel, type View } from "./shared";

export function settingsView(): View {
  const s0 = store.current();
  const save = (mutate: (s: Snapshot["settings"]) => void) => void store.updateSettings(mutate);
  const saveSoon = debounce(save, 120);

  // --- Steadying ---
  const steadiness = slider({
    label: "Steadiness",
    min: 0,
    max: 100,
    step: 1,
    value: s0.settings.filter.steadiness,
    format: (v) => `${Math.round(v)} · ${steadinessLabel(v)}`,
    scale: ["Lighter, quicker", "Steadier, smoother"],
    hint: "Higher removes more shake but makes the pointer a little slower to start moving.",
    onChange: (v) =>
      save((s) => {
        s.filter.steadiness = v;
        s.filter.custom = null;
      }),
  });

  const custom = () => ({ ...store.current().effective_filter });
  const minCutoff = slider({
    label: "Resting cutoff",
    min: 0.1,
    max: 5,
    step: 0.05,
    value: s0.effective_filter.min_cutoff,
    format: (v) => `${v.toFixed(2)} Hz`,
    hint: "How much smoothing when your hand is still. Lower is steadier.",
    onChange: (v) => saveSoon((s) => (s.filter.custom = { ...custom(), min_cutoff: v })),
  });
  const beta = slider({
    label: "Speed response",
    min: 0,
    max: 0.05,
    step: 0.0005,
    value: s0.effective_filter.beta,
    format: (v) => v.toFixed(4),
    hint: "How quickly smoothing eases off during fast, deliberate moves. Higher is snappier.",
    onChange: (v) => saveSoon((s) => (s.filter.custom = { ...custom(), beta: v })),
  });
  const dCutoff = slider({
    label: "Speed smoothing",
    min: 0.2,
    max: 5,
    step: 0.1,
    value: s0.effective_filter.d_cutoff,
    format: (v) => `${v.toFixed(1)} Hz`,
    hint: "How the speed is measured. Lower keeps the tremor itself from loosening the filter.",
    onChange: (v) => saveSoon((s) => (s.filter.custom = { ...custom(), d_cutoff: v })),
  });
  const customNote = h(
    "div",
    { class: "banner info" },
    h(
      "div",
      { class: "body" },
      h("p", null, "These exact values (from calibration or set by hand) are in use instead of the steadiness slider."),
      h(
        "div",
        { class: "btn-row" },
        h("button", { class: "btn", onclick: () => save((s) => (s.filter.custom = null)) }, "Use the slider instead"),
      ),
    ),
  );
  const rate = h(
    "select",
    {
      "aria-label": "Update rate",
      onchange: (e: Event) => save((s) => (s.tick_hz = Number((e.target as HTMLSelectElement).value))),
    },
    [125, 250, 500, 1000].map((hz) => h("option", { value: String(hz) }, `${hz} times a second${hz === 250 ? " (recommended)" : ""}`)),
  );

  // --- Clicking ---
  const anchor = toggleRow(
    "Click lock",
    "Holds the pointer still for a moment when you press a button.",
    s0.settings.click.anchor_enabled,
    (on) => save((s) => (s.click.anchor_enabled = on)),
  );
  const anchorMs = slider({
    label: "Hold the pointer for up to",
    min: 50,
    max: 1000,
    step: 10,
    value: s0.settings.click.anchor_ms,
    format: (v) => `${Math.round(v)} ms`,
    hint: "Long enough for a click, short enough that dragging still feels natural.",
    onChange: (v) => save((s) => (s.click.anchor_ms = Math.round(v))),
  });
  const anchorRadius = slider({
    label: "Start a drag after moving",
    min: 2,
    max: 80,
    step: 1,
    value: s0.settings.click.anchor_radius_px,
    format: (v) => `${Math.round(v)} px`,
    hint: "Moving further than this while pressed starts dragging straight away.",
    onChange: (v) => save((s) => (s.click.anchor_radius_px = v)),
  });
  const debounceMs = slider({
    label: "Ignore a second press within",
    min: 0,
    max: 150,
    step: 5,
    value: s0.settings.click.debounce_ms,
    format: (v) => (v === 0 ? "Off" : `${Math.round(v)} ms`),
    hint: "Stops a shaky finger from clicking twice by accident. Keep it short so double-clicks still work.",
    onChange: (v) => save((s) => (s.click.debounce_ms = Math.round(v))),
  });

  // --- General ---
  const login = toggleRow(
    "Start with my computer",
    "Open Calmcursor in the background when you log in.",
    s0.settings.launch_at_login,
    (on) => save((s) => (s.launch_at_login = on)),
  );
  const hidden = toggleRow(
    "Start hidden",
    "Open to the tray / menu bar instead of showing this window.",
    s0.settings.start_minimized,
    (on) => save((s) => (s.start_minimized = on)),
  );
  const shortcutInput = h("input", {
    type: "text",
    value: s0.settings.pause_shortcut,
    spellcheck: false,
    "aria-label": "Pause shortcut",
    placeholder: "e.g. CommandOrControl+Alt+Shift+S",
  });
  const shortcutHint = h("p", { class: "field-hint" });
  const shortcutSave = h(
    "button",
    {
      class: "btn",
      onclick: async () => {
        await store.updateSettings((s) => (s.pause_shortcut = shortcutInput.value.trim()));
        const err = store.current().shortcut_error;
        toast(err ? "That shortcut could not be used." : "Shortcut saved.");
      },
    },
    "Save shortcut",
  );

  const el = h(
    "div",
    { class: "view-inner" },
    h(
      "header",
      { class: "view-head" },
      h("div", null, h("h1", null, "Settings"), h("p", null, "Changes apply at once and are saved automatically.")),
    ),
    h(
      "section",
      { class: "card" },
      h("h2", null, "Steadying"),
      steadiness.el,
      customNote,
      h("details", null, h("summary", { class: "btn ghost" }, "Advanced filter settings"),
        h("div", { class: "card", style: { boxShadow: "none", marginTop: "12px" } },
          h("p", { class: "muted small" }, "Calmcursor uses the One Euro filter, an adaptive smoothing filter. Changing these switches to exact values."),
          minCutoff.el, beta.el, dCutoff.el,
          h("div", { class: "field" }, h("label", { class: "field-head" }, h("span", { style: { fontWeight: "650" } }, "Update rate")), rate),
        ),
      ),
    ),
    h("section", { class: "card" }, h("h2", null, "Clicking"), anchor.el, anchorMs.el, anchorRadius.el, h("div", { class: "divider" }), debounceMs.el),
    h(
      "section",
      { class: "card" },
      h("h2", null, "General"),
      login.el,
      hidden.el,
      h("div", { class: "divider" }),
      h(
        "div",
        { class: "field" },
        h("div", { class: "field-head" }, h("label", null, "Pause shortcut")),
        h("div", { class: "btn-row", style: { flexWrap: "nowrap" } }, shortcutInput, shortcutSave),
        shortcutHint,
      ),
    ),
    h(
      "section",
      { class: "card" },
      h("h2", null, "Start over"),
      h("p", { class: "muted" }, "Puts every setting back to its default. Your last calibration is forgotten too."),
      h(
        "div",
        { class: "btn-row" },
        h(
          "button",
          {
            class: "btn",
            onclick: async () => {
              if (!window.confirm("Reset all settings to their defaults?")) return;
              store.set(await api.resetSettings());
              shortcutInput.value = store.current().settings.pause_shortcut;
              toast("Settings reset.");
            },
          },
          "Reset all settings",
        ),
      ),
    ),
  );

  function update(s: Snapshot) {
    steadiness.set(s.settings.filter.steadiness);
    customNote.hidden = s.settings.filter.custom === null;
    minCutoff.set(s.effective_filter.min_cutoff);
    beta.set(s.effective_filter.beta);
    dCutoff.set(s.effective_filter.d_cutoff);
    rate.value = String(s.settings.tick_hz);
    anchor.set(s.settings.click.anchor_enabled);
    anchorMs.set(s.settings.click.anchor_ms);
    anchorRadius.set(s.settings.click.anchor_radius_px);
    debounceMs.set(s.settings.click.debounce_ms);
    login.set(s.settings.launch_at_login);
    hidden.set(s.settings.start_minimized);
    shortcutHint.textContent = s.shortcut_error
      ? s.shortcut_error
      : s.settings.pause_shortcut
        ? `Now: ${formatShortcut(s.settings.pause_shortcut, s.os)}. Leave empty to turn the shortcut off.`
        : "No shortcut set.";
  }

  return { el, update };
}
