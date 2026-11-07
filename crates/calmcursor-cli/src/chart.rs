//! Renders the "before and after" SVG chart used in the README.

use calmcursor_core::simulate::TremorModel;
use calmcursor_core::tremor;
use calmcursor_core::{TraceSample, Vec2};
use std::fmt::Write;

const RAW: &str = "#e4572e";
const SMOOTH: &str = "#1f8a7a";
const INK: &str = "#11221f";
const MUTED: &str = "#5b6b68";
const LINE: &str = "#e3e9e7";
const WHITE: &str = "#ffffff";
const CARD: &str = "#fbfcfc";
const FONT: &str =
    "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif";

/// How much shake remains when the hand is (nearly) at rest.
#[derive(Debug, Clone, Copy)]
pub struct Summary {
    pub raw_jitter: f64,
    pub smooth_jitter: f64,
}

impl Summary {
    pub fn from_trace(trace: &[TraceSample], fs: f64) -> Self {
        let raw: Vec<Vec2> = trace.iter().map(|s| s.raw).collect();
        let out: Vec<Vec2> = trace.iter().map(|s| s.out).collect();
        if raw.len() < 8 {
            return Self {
                raw_jitter: 0.0,
                smooth_jitter: 0.0,
            };
        }
        let trend = tremor::lowpass_2d(&raw, fs, tremor::TREND_CUTOFF_HZ);
        let raw_hp = tremor::high_pass(&raw, fs);
        let out_hp = tremor::high_pass(&out, fs);
        // "At rest" = the intended (slow) movement is under 60 px/s.
        let mut r = Vec::new();
        let mut o = Vec::new();
        for i in 1..trend.len() {
            let speed = (trend[i] - trend[i - 1]).length() * fs;
            if speed < 60.0 {
                r.push(raw_hp[i]);
                o.push(out_hp[i]);
            }
        }
        Self {
            raw_jitter: tremor::rms(&r),
            smooth_jitter: tremor::rms(&o),
        }
    }

    pub fn reduction_percent(&self) -> f64 {
        if self.raw_jitter > 0.0 {
            ((1.0 - self.smooth_jitter / self.raw_jitter) * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        }
    }
}

fn polyline(points: impl Iterator<Item = (f64, f64)>) -> String {
    let mut s = String::new();
    for (i, (x, y)) in points.enumerate() {
        let _ = write!(s, "{}{:.1},{:.1}", if i == 0 { "" } else { " " }, x, y);
    }
    s
}

/// Draws the two-panel chart: the whole path, and a close-up of one click.
pub fn render(
    trace: &[TraceSample],
    summary: &Summary,
    model: TremorModel,
    steadiness: u8,
    area_w: f64,
    area_h: f64,
) -> String {
    let (w, h) = (1200.0, 640.0);
    let mut svg = String::new();
    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" font-family="{FONT}" role="img" aria-labelledby="title desc">
<title id="title">Same shaky hand, before and after Calmcursor</title>
<desc id="desc">A simulated {freq:.1} Hz tremor of about {amp:.0} pixels. The raw path zig-zags; the steadied path is smooth and holds still during clicks.</desc>
<rect x="1" y="1" width="{bw}" height="{bh}" rx="22" fill="{CARD}" stroke="{LINE}" stroke-width="2"/>
<text x="44" y="64" font-size="28" font-weight="700" fill="{INK}">Same shaky hand, before and after Calmcursor</text>
<text x="44" y="96" font-size="16" fill="{MUTED}">A simulated {freq:.1} Hz tremor (about ±{amp:.0} px) run through the real Calmcursor engine at steadiness {steadiness}.</text>
"#,
        freq = model.frequency_hz,
        amp = model.amplitude_px,
        bw = w - 2.0,
        bh = h - 2.0,
    );

    // Left: the whole path, fitted into the panel.
    let (px, py, pw, ph) = (44.0, 124.0, 690.0, 430.0);
    let scale = (pw / area_w).min(ph / area_h);
    let ox = px + (pw - area_w * scale) / 2.0;
    let oy = py + (ph - area_h * scale) / 2.0;
    let map = |p: Vec2| (ox + p.x * scale, oy + p.y * scale);
    let _ = write!(
        svg,
        r#"<rect x="{px}" y="{py}" width="{pw}" height="{ph}" rx="16" fill="{WHITE}" stroke="{LINE}"/>
<polyline points="{raw}" fill="none" stroke="{RAW}" stroke-width="1.6" stroke-opacity="0.8" stroke-linejoin="round"/>
<polyline points="{out}" fill="none" stroke="{SMOOTH}" stroke-width="3.4" stroke-linecap="round" stroke-linejoin="round"/>
"#,
        raw = polyline(trace.iter().map(|s| map(s.raw))),
        out = polyline(trace.iter().map(|s| map(s.out))),
    );
    let presses: Vec<&TraceSample> = trace
        .iter()
        .filter(|s| s.flags & TraceSample::PRESS != 0)
        .collect();
    for s in &presses {
        let (x, y) = map(s.out);
        let _ = writeln!(
            svg,
            r#"<circle cx="{x:.1}" cy="{y:.1}" r="9" fill="{WHITE}" stroke="{SMOOTH}" stroke-width="3"/>"#
        );
    }

    // Right: a close-up around the first click.
    let (zx, zy, zs) = (772.0, 124.0, 384.0);
    let _ = write!(
        svg,
        r#"<rect x="{zx}" y="{zy}" width="{zs}" height="{zs}" rx="16" fill="{WHITE}" stroke="{LINE}"/>
<clipPath id="zoom"><rect x="{zx}" y="{zy}" width="{zs}" height="{zs}" rx="16"/></clipPath>
"#
    );
    if let Some(press) = presses.first() {
        let half = 26.0;
        let k = zs / (2.0 * half);
        let c = press.out;
        let zmap = |p: Vec2| (zx + (p.x - c.x + half) * k, zy + (p.y - c.y + half) * k);
        let window: Vec<&TraceSample> = trace
            .iter()
            .filter(|s| s.t >= press.t - 0.9 && s.t <= press.t + 0.6)
            .collect();
        let _ = write!(
            svg,
            r#"<g clip-path="url(#zoom)">
<polyline points="{raw}" fill="none" stroke="{RAW}" stroke-width="2.2" stroke-opacity="0.85" stroke-linejoin="round"/>
<polyline points="{out}" fill="none" stroke="{SMOOTH}" stroke-width="5" stroke-linecap="round" stroke-linejoin="round"/>
</g>
<circle cx="{cx:.1}" cy="{cy:.1}" r="16" fill="none" stroke="{SMOOTH}" stroke-width="4"/>
"#,
            raw = polyline(window.iter().map(|s| zmap(s.raw))),
            out = polyline(window.iter().map(|s| zmap(s.out))),
            cx = zx + zs / 2.0,
            cy = zy + zs / 2.0,
        );
    }
    let _ = writeln!(
        svg,
        r#"<text x="{tx}" y="{ty}" font-size="15" fill="{MUTED}">Close-up of one click (×7): the pointer holds still.</text>"#,
        tx = zx + 4.0,
        ty = zy + zs + 28.0
    );

    // Legend and result.
    let ly = 596.0;
    let _ = write!(
        svg,
        r#"<line x1="44" y1="{ly}" x2="80" y2="{ly}" stroke="{RAW}" stroke-width="3"/>
<text x="90" y="{lt}" font-size="16" fill="{INK}">Raw hand movement</text>
<line x1="270" y1="{ly}" x2="306" y2="{ly}" stroke="{SMOOTH}" stroke-width="5" stroke-linecap="round"/>
<text x="316" y="{lt}" font-size="16" fill="{INK}">What Calmcursor shows</text>
<circle cx="530" cy="{ly}" r="8" fill="{WHITE}" stroke="{SMOOTH}" stroke-width="3"/>
<text x="546" y="{lt}" font-size="16" fill="{INK}">Click</text>
<text x="1156" y="{lt}" font-size="17" font-weight="700" fill="{SMOOTH}" text-anchor="end">Shake at rest: {r:.1} px → {s:.1} px ({p:.0}% less)</text>
</svg>
"#,
        lt = ly + 5.0,
        r = summary.raw_jitter,
        s = summary.smooth_jitter,
        p = summary.reduction_percent(),
    );
    svg
}

#[cfg(test)]
mod tests {
    use super::*;
    use calmcursor_core::simulate::Demo;
    use calmcursor_core::Settings;

    #[test]
    fn renders_valid_looking_svg() {
        let mut demo = Demo::new(
            &Settings::default(),
            TremorModel::moderate(),
            900.0,
            560.0,
            1,
        );
        demo.advance(1.0);
        demo.drain_trace();
        let mut trace = Vec::new();
        for _ in 0..400 {
            demo.advance(0.01);
            trace.extend(demo.drain_trace());
        }
        let summary = Summary::from_trace(&trace, 250.0);
        assert!(summary.reduction_percent() > 50.0, "{summary:?}");
        let svg = render(&trace, &summary, TremorModel::moderate(), 50, 900.0, 560.0);
        assert!(svg.starts_with("<svg") && svg.trim_end().ends_with("</svg>"));
        assert!(svg.contains("polyline"));
    }
}
