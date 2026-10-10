//! Color schemes: a strong pastel sky over a simple drawn landscape (inline
//! SVG, no images), each with an accent from across the color wheel in a deep
//! shade (white text passes 4.5:1 on every one). Waves and clouds drift slowly;
//! `motion-safe:` stops them when the phone asks for less motion.

use std::fmt::Write;

/// Shades 50, 100, 200, 700, 800, 900 of a Tailwind color.
type Palette = [&'static str; 6];

const SKY: Palette = [
    "#f0f9ff", "#e0f2fe", "#bae6fd", "#0369a1", "#075985", "#0c4a6e",
];
const PINK: Palette = [
    "#fdf2f8", "#fce7f3", "#fbcfe8", "#be185d", "#9d174d", "#831843",
];
const VIOLET: Palette = [
    "#f5f3ff", "#ede9fe", "#ddd6fe", "#6d28d9", "#5b21b6", "#4c1d95",
];
const GREEN: Palette = [
    "#f0fdf4", "#dcfce7", "#bbf7d0", "#15803d", "#166534", "#14532d",
];
const FUCHSIA: Palette = [
    "#fdf4ff", "#fae8ff", "#f5d0fe", "#a21caf", "#86198f", "#701a75",
];
const INDIGO: Palette = [
    "#eef2ff", "#e0e7ff", "#c7d2fe", "#4338ca", "#3730a3", "#312e81",
];
const AMBER: Palette = [
    "#fffbeb", "#fef3c7", "#fde68a", "#b45309", "#92400e", "#78350f",
];
const ROSE: Palette = [
    "#fff1f2", "#ffe4e6", "#fecdd3", "#be123c", "#9f1239", "#881337",
];
const TEAL: Palette = [
    "#f0fdfa", "#ccfbf1", "#99f6e4", "#0f766e", "#115e59", "#134e4a",
];
const LIME: Palette = [
    "#f7fee7", "#ecfccb", "#d9f99d", "#4d7c0f", "#3f6212", "#365314",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Scheme {
    pub id: &'static str,
    pub name: &'static str,
    /// Tailwind classes for the page background.
    pub sky: &'static str,
    accent: Palette,
    scene: Scene,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scene {
    Waves([&'static str; 3]),
    Slopes([&'static str; 3]),
    Peaks([&'static str; 3]),
    Woods([&'static str; 3]),
    Fjord([&'static str; 3]),
    Clouds,
    None,
}

pub const SCHEMES: [Scheme; 16] = [
    Scheme {
        id: "ocean",
        name: "Ocean",
        sky: "bg-linear-to-b from-sky-200 to-cyan-100",
        accent: ROSE,
        scene: Scene::Waves(["#7dd3fc", "#38bdf8", "#0ea5e9"]),
    },
    Scheme {
        id: "sunset",
        name: "Sunset",
        sky: "bg-linear-to-b from-orange-200 to-rose-200",
        accent: SKY,
        scene: Scene::Slopes(["#fdba74", "#fb923c", "#f97316"]),
    },
    Scheme {
        id: "lavender",
        name: "Lavender",
        sky: "bg-linear-to-b from-violet-200 to-indigo-100",
        accent: LIME,
        scene: Scene::Peaks(["#c4b5fd", "#a78bfa", "#8b5cf6"]),
    },
    Scheme {
        id: "pinewood",
        name: "Pinewood",
        sky: "bg-linear-to-b from-emerald-200 to-teal-100",
        accent: PINK,
        scene: Scene::Woods(["#6ee7b7", "#34d399", "#10b981"]),
    },
    Scheme {
        id: "spring",
        name: "Spring",
        sky: "bg-linear-to-b from-lime-200 to-emerald-100",
        accent: VIOLET,
        scene: Scene::Slopes(["#bef264", "#a3e635", "#84cc16"]),
    },
    Scheme {
        id: "lagoon",
        name: "Lagoon",
        sky: "bg-linear-to-b from-teal-200 to-cyan-100",
        accent: FUCHSIA,
        scene: Scene::Waves(["#5eead4", "#2dd4bf", "#14b8a6"]),
    },
    Scheme {
        id: "glacier",
        name: "Glacier",
        sky: "bg-linear-to-b from-sky-200 to-indigo-100",
        accent: AMBER,
        scene: Scene::Peaks(["#bae6fd", "#7dd3fc", "#38bdf8"]),
    },
    Scheme {
        id: "heather",
        name: "Heather",
        sky: "bg-linear-to-b from-fuchsia-200 to-pink-100",
        accent: GREEN,
        scene: Scene::Slopes(["#f0abfc", "#e879f9", "#d946ef"]),
    },
    Scheme {
        id: "fjord",
        name: "Fjord",
        sky: "bg-linear-to-b from-indigo-200 to-sky-100",
        accent: AMBER,
        scene: Scene::Fjord(["#a5b4fc", "#818cf8", "#7dd3fc"]),
    },
    Scheme {
        id: "jungle",
        name: "Jungle",
        sky: "bg-linear-to-b from-lime-200 to-green-100",
        accent: FUCHSIA,
        scene: Scene::Woods(["#86efac", "#4ade80", "#22c55e"]),
    },
    Scheme {
        id: "canyon",
        name: "Canyon",
        sky: "bg-linear-to-b from-amber-200 to-orange-100",
        accent: INDIGO,
        scene: Scene::Peaks(["#fcd34d", "#fbbf24", "#f59e0b"]),
    },
    Scheme {
        id: "rose",
        name: "Rose",
        sky: "bg-linear-to-b from-rose-200 to-pink-100",
        accent: TEAL,
        scene: Scene::Slopes(["#fda4af", "#fb7185", "#f43f5e"]),
    },
    Scheme {
        id: "seafoam",
        name: "Seafoam",
        sky: "bg-linear-to-b from-emerald-200 to-cyan-100",
        accent: ROSE,
        scene: Scene::Waves(["#6ee7b7", "#34d399", "#10b981"]),
    },
    Scheme {
        id: "clouds",
        name: "Clouds",
        sky: "bg-linear-to-b from-sky-100 to-sky-50",
        accent: PINK,
        scene: Scene::Clouds,
    },
    Scheme {
        id: "blueclouds",
        name: "Blue clouds",
        sky: "bg-linear-to-b from-sky-300 to-sky-100",
        accent: AMBER,
        scene: Scene::Clouds,
    },
    Scheme {
        id: "slate",
        name: "Slate",
        sky: "bg-linear-to-b from-slate-200 to-slate-100",
        accent: FUCHSIA,
        scene: Scene::None,
    },
];

pub fn scheme(id: &str) -> Scheme {
    SCHEMES
        .iter()
        .copied()
        .find(|scheme| scheme.id == id)
        .unwrap_or(SCHEMES[0])
}

impl Scheme {
    /// The accent's shades as CSS variables, for the root element's `style`.
    pub fn accent_vars(&self) -> String {
        ["50", "100", "200", "700", "800", "900"]
            .iter()
            .zip(self.accent)
            .fold(String::new(), |mut css, (step, hex)| {
                let _ = write!(css, "--color-accent-{step}:{hex};");
                css
            })
    }

    /// The drawing at the bottom of the screen, as SVG markup.
    pub fn scene_svg(&self) -> String {
        match self.scene {
            Scene::Waves([a, b, c]) => svg(&format!(
                "{}{}{}{}",
                drift(Drift::Far, r##"<path d="M90 70 q8 -7 16 0 q8 -7 16 0 M140 50 q6 -5 12 0 q6 -5 12 0" fill="none" stroke="#ffffff" stroke-width="2" stroke-linecap="round" opacity=".8"/>"##),
                drift(Drift::Far, &path("M-40 190 C30 178 110 200 180 190 S320 178 470 192 V320 H-40Z", a, 1.0)),
                drift(Drift::Mid, &path("M-40 230 C40 213 140 240 210 228 S360 213 470 232 V320 H-40Z", b, 0.8)),
                drift(Drift::Near, &path("M-40 275 C60 258 160 285 240 272 S380 260 470 278 V320 H-40Z", c, 0.6)),
            )),
            Scene::Slopes([a, b, c]) => svg(&format!(
                "{}{}{}",
                path("M0 215 C110 170 200 230 300 200 S400 175 430 185 V320 H0Z", a, 1.0),
                path("M0 255 C90 230 190 270 280 245 S390 230 430 240 V320 H0Z", b, 0.85),
                path("M0 295 C120 275 230 300 330 285 S410 280 430 284 V320 H0Z", c, 0.6),
            )),
            Scene::Peaks([a, b, c]) => svg(&format!(
                "{}{}{}{}",
                path("M0 230 L70 140 L120 190 L190 90 L260 180 L310 130 L430 230 V320 H0Z", a, 1.0),
                path("M190 90 L212 118 L200 114 L190 124 L180 113 L170 116Z", "#ffffff", 0.95),
                path("M0 260 L90 190 L160 240 L240 170 L330 245 L380 210 L430 250 V320 H0Z", b, 0.85),
                path("M0 290 C110 265 220 275 430 280 V320 H0Z", c, 0.6),
            )),
            Scene::Woods([a, b, c]) => svg(&format!("{}{}{}", pines(250.0, 110.0, 9, a, 1.0, 7), pines(285.0, 95.0, 11, b, 0.85, 5), pines(320.0, 80.0, 13, c, 0.6, 3))),
            Scene::Fjord([a, b, water]) => svg(&format!(
                "{}{}{}{}{}",
                path("M0 210 L60 120 L110 170 L180 80 L250 160 L320 110 L430 200 V250 H0Z", a, 1.0),
                path("M180 80 L200 106 L189 102 L180 112 L171 101 L162 104Z", "#ffffff", 0.95),
                path("M0 235 L80 180 L150 225 L230 165 L320 230 L430 210 V260 H0Z", b, 0.85),
                drift(Drift::Mid, &path("M-40 250 H470 V320 H-40Z", water, 0.8)),
                drift(Drift::Near, r##"<g stroke="#ffffff" stroke-width="3" stroke-linecap="round" opacity=".7"><path d="M150 272 h120"/><path d="M175 292 h70"/></g>"##),
            )),
            Scene::Clouds => svg(&[(70, 90, 1.0, Drift::Far), (300, 60, 0.8, Drift::Mid), (220, 170, 1.2, Drift::Near), (60, 230, 0.9, Drift::Mid), (360, 250, 1.0, Drift::Far)]
                .iter()
                .map(|(x, y, k, speed)| drift(*speed, &format!(r##"<g transform="translate({x} {y}) scale({k})" fill="#ffffff"><circle cx="0" cy="0" r="18"/><circle cx="20" cy="-8" r="22"/><circle cx="42" cy="0" r="16"/><rect x="0" y="-2" width="42" height="18" rx="9"/></g>"##)))
                .collect::<String>()),
            Scene::None => String::new(),
        }
    }
}

#[derive(Clone, Copy)]
enum Drift {
    Far,
    Mid,
    Near,
}

fn drift(speed: Drift, body: &str) -> String {
    // Written out in full so Tailwind finds the class names.
    let class = match speed {
        Drift::Far => "motion-safe:animate-drift-far",
        Drift::Mid => "motion-safe:animate-drift-mid",
        Drift::Near => "motion-safe:animate-drift-near",
    };
    format!(r##"<g class="{class}">{body}</g>"##)
}

fn path(d: &str, fill: &str, opacity: f32) -> String {
    format!(r##"<path d="{d}" fill="{fill}" opacity="{opacity}"/>"##)
}

fn svg(body: &str) -> String {
    format!(
        r##"<svg viewBox="0 0 430 320" preserveAspectRatio="xMidYMax slice" class="size-full">{body}</svg>"##
    )
}

/// A row of pine trees with slightly varied heights.
fn pines(y: f32, h: f32, count: u32, fill: &str, opacity: f32, seed: u32) -> String {
    (0..count)
        .map(|i| {
            let x = (i as f32 + 0.5) * (430.0 / count as f32) + ((i * seed) % 17) as f32 - 8.0;
            let t = h * (0.75 + ((i * seed) % 5) as f32 / 10.0);
            path(
                &format!(
                    "M{x} {} L{} {y} L{} {y} Z",
                    y - t,
                    x + t * 0.32,
                    x - t * 0.32
                ),
                fill,
                opacity,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scheme_has_a_unique_id_and_a_full_accent() {
        let mut ids: Vec<_> = SCHEMES.iter().map(|scheme| scheme.id).collect();
        ids.dedup();
        assert_eq!(ids.len(), SCHEMES.len());
        for scheme in SCHEMES {
            assert_eq!(
                scheme.accent_vars().matches("--color-accent-").count(),
                6,
                "{}",
                scheme.id
            );
        }
        assert_eq!(scheme("nope").id, "ocean");
    }
}
