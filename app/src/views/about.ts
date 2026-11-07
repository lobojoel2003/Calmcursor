// About: version, build number, and where to get help.

import { api, REPO_URL, type Snapshot } from "../api";
import { h, icon } from "../ui";
import type { View } from "./shared";

export function aboutView(): View {
  const details = h("dl", { class: "kv" });
  const link = (label: string, url: string, iconName = "external") =>
    h("button", { class: "btn", onclick: () => void api.openUrl(url) }, icon(iconName), label);

  const el = h(
    "div",
    { class: "view-inner" },
    h(
      "section",
      { class: "card" },
      h(
        "div",
        { class: "about-hero" },
        h("img", { src: "/logo.svg", alt: "" }),
        h(
          "div",
          null,
          h("h1", null, "Calmcursor"),
          h("p", { class: "muted" }, "A tremor-steadying pointer for Windows, macOS and Linux. Free and open source."),
        ),
      ),
      h(
        "div",
        { class: "btn-row" },
        link("Project page", REPO_URL, "github"),
        link("Report a problem", `${REPO_URL}/issues/new/choose`),
        link("Release notes", `${REPO_URL}/releases`),
      ),
    ),
    h("section", { class: "card" }, h("h2", null, "This build"), details),
    h(
      "section",
      { class: "card" },
      h("h2", null, "If something goes wrong"),
      h(
        "p",
        null,
        "Pause steadying with the shortcut or from the tray icon, or quit Calmcursor. The moment it stops, your mouse behaves exactly as before. Calmcursor never changes system mouse settings.",
      ),
    ),
    h(
      "section",
      { class: "card" },
      h("h2", null, "Credits"),
      h(
        "p",
        null,
        "Smoothing uses the One Euro filter by Géry Casiez, Nicolas Roussel and Daniel Vogel (CHI 2012). Calmcursor is licensed under the GNU GPL v3 or later.",
      ),
      h(
        "div",
        { class: "btn-row" },
        link("About the One Euro filter", "https://gery.casiez.net/1euro/"),
        link("GPL v3 license", "https://www.gnu.org/licenses/gpl-3.0.html"),
      ),
      h(
        "p",
        { class: "small muted" },
        "Calmcursor is an everyday aid, not a medical device. Its tremor measurements are for tuning only.",
      ),
    ),
  );

  function update(s: Snapshot) {
    const b = s.build;
    const rows: [string, string][] = [
      ["Version", b.version],
      ["Build", b.build_number],
      ["Commit", b.git_sha],
      ["Built", b.build_date],
      ["Channel", b.channel],
      ["Platform", b.target],
      ["Pointer hook", s.backend],
      ["Devices", s.devices.length ? s.devices.join(", ") : s.os === "linux" ? "none yet" : "all pointing devices"],
      ["Settings file", s.settings_path ?? "–"],
    ];
    details.replaceChildren(
      ...rows.flatMap(([k, v]) => [h("dt", null, k), h("dd", { class: k === "Settings file" || k === "Commit" ? "mono" : "" }, v)]),
    );
  }

  return { el, update };
}

