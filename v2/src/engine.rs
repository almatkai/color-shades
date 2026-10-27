use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub const LEVELS: [u16; 11] = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    #[serde(rename = "creationDate")]
    pub creation_date: String,
    pub name: String,
    pub colors: BTreeMap<u16, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Generator {
    cmy: [i32; 3],
    rgb: [i32; 4],
}
impl Default for Generator {
    fn default() -> Self {
        Self {
            cmy: [180, 300, 60],
            rgb: [360, 240, 120, 0],
        }
    }
}

#[derive(Deserialize)]
struct Name {
    name: String,
    hex: String,
    slug: String,
    #[serde(skip)]
    lab: [f64; 3],
}
fn names() -> &'static [Name] {
    static NAMES: OnceLock<Vec<Name>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names: Vec<Name> =
            serde_json::from_str(include_str!("../vendor/ntc.json")).expect("bundled NTC data");
        for n in &mut names {
            n.lab = lab(parse_rgb(&n.hex));
        }
        names
    })
}

/// JS String.replace removes the first hash; padEnd uses UTF-16 length.
/// The three-character regex expansion excludes JS line terminators.
pub fn normalize(input: &str) -> String {
    let input = input.replacen('#', "", 1);
    let len = input.encode_utf16().count();
    let body = if len == 3 {
        String::from_utf16_lossy(
            &input
                .encode_utf16()
                .flat_map(|c| {
                    if [10, 13, 0x2028, 0x2029].contains(&c) {
                        vec![c]
                    } else {
                        vec![c, c]
                    }
                })
                .collect::<Vec<_>>(),
        )
    } else {
        format!("{}{}", input, "0".repeat(6usize.saturating_sub(len)))
    };
    format!("#{body}")
}

fn parse_rgb(hex: &str) -> [f64; 3] {
    let n = u32::from_str_radix(&hex[..6], 16).expect("validated bundled/input hex");
    [
        ((n >> 16) & 255) as f64,
        ((n >> 8) & 255) as f64,
        (n & 255) as f64,
    ]
}
fn named_rgb(hex: &str) -> Result<[f64; 3], String> {
    // chroma-js 1.4.1 accepts six/eight hex digits here. Alpha does not
    // participate in the default Lab distance or color-convert's shade math.
    let body = hex.strip_prefix('#').unwrap_or(hex);
    if [6, 8].contains(&body.len()) && body.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(parse_rgb(body));
    }
    Err(format!("unknown color: {hex}"))
}

// Exact chroma-js 1.4.1 D65 Lab constants and operation order (BSD-3-Clause).
fn lab(rgb: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(|x| {
        let x = x / 255.0;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    });
    let f = |x: f64| {
        if x > 0.008856452 {
            x.powf(1.0 / 3.0)
        } else {
            x / 0.12841855 + 0.137931034
        }
    };
    let x = f((0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.950470);
    let y = f(0.2126729 * r + 0.7151522 * g + 0.0721750 * b);
    let z = f((0.0193339 * r + 0.1191920 * g + 0.9503041 * b) / 1.088830);
    [116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z)]
}
fn nearest(rgb: [f64; 3]) -> &'static Name {
    let p = lab(rgb);
    let distance = |n: &Name| {
        let d = [p[0] - n.lab[0], p[1] - n.lab[1], p[2] - n.lab[2]];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    };
    // Strict < retains original NTC order on equal distances (JS stable sort).
    let mut best = &names()[0];
    let mut min = distance(best);
    for n in &names()[1..] {
        let d = distance(n);
        if d < min {
            best = n;
            min = d;
        }
    }
    best
}
fn js_round(v: f64) -> f64 {
    (v + 0.5).floor()
}
fn hsv(rgb: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(|x| x / 255.0);
    let v = r.max(g).max(b);
    let diff = v - r.min(g).min(b);
    let (mut h, s) = if diff == 0.0 {
        (0.0, 0.0)
    } else {
        let d = |c: f64| (v - c) / 6.0 / diff + 0.5;
        (
            if r == v {
                d(b) - d(g)
            } else if g == v {
                1.0 / 3.0 + d(r) - d(b)
            } else {
                2.0 / 3.0 + d(g) - d(r)
            },
            diff / v,
        )
    };
    if h < 0.0 {
        h += 1.0;
    } else if h > 1.0 {
        h -= 1.0;
    }
    [h * 360.0, s * 100.0, v * 100.0].map(js_round)
}
fn hex_hsv([h, s, v]: [f64; 3]) -> String {
    let h = h / 60.0;
    let s = s / 100.0;
    let v = v / 100.0;
    let hi = h.floor() as i32 % 6;
    let f = h - h.floor();
    let p = 255.0 * v * (1.0 - s);
    let q = 255.0 * v * (1.0 - s * f);
    let t = 255.0 * v * (1.0 - s * (1.0 - f));
    let v = v * 255.0;
    let rgb = match hi {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        5 => [v, p, q],
        _ => unreachable!("source hue range"),
    };
    let [r, g, b] = rgb.map(|x| (js_round(x) as i32 & 255) as u8);
    format!("#{r:02X}{g:02X}{b:02X}")
}
fn shift(hues: &mut [i32], h: f64, intensity: f64) -> f64 {
    hues.sort_by(|a, b| {
        (f64::from(*a) - h)
            .abs()
            .total_cmp(&(f64::from(*b) - h).abs())
    });
    js_round(intensity * (f64::from(hues[0]) - h) * 0.5)
}
impl Generator {
    pub fn generate(&mut self, input: &str, date: &str) -> Result<Palette, String> {
        let mut cmy = self.cmy;
        let mut rgb = self.rgb;
        cmy.sort();
        rgb.sort();
        if cmy != [60, 180, 300] || rgb != [0, 120, 240, 360] {
            return Err("Corrupt generator hue state".into());
        }
        let full = normalize(input);
        let rgb = named_rgb(&full)?;
        let name = nearest(rgb).name.clone();
        // color-convert searches for the first 6/3 hex substring; accepted
        // chroma inputs have the first six digits as RGB, including RGBA.
        let [h, s, v] = hsv(rgb);
        let mut colors = BTreeMap::new();
        colors.insert(500, full);
        for (level, i) in [(50, 0.95), (100, 0.9), (200, 0.75), (300, 0.6), (400, 0.3)] {
            let hue = h + shift(&mut self.cmy, h, i);
            colors.insert(
                level,
                hex_hsv([hue, s - js_round(s * i), v + js_round((100.0 - v) * i)]),
            );
        }
        for (level, i) in [
            (600, 0.9),
            (700, 0.75),
            (800, 0.6),
            (900, 0.45),
            (950, 0.29),
        ] {
            let i = 1.0 - i;
            let hue = h + shift(&mut self.rgb, h, i);
            colors.insert(
                level,
                hex_hsv([hue, s + js_round((100.0 - s) * i), v - js_round(v * i)]),
            );
        }
        Ok(Palette {
            creation_date: date.into(),
            name,
            colors,
        })
    }
}
impl Palette {
    pub fn slug(&self) -> Result<&'static str, String> {
        names()
            .iter()
            .find(|n| n.name == self.name)
            .map(|n| n.slug.as_str())
            .ok_or("Unknown NTC name".into())
    }
    pub fn variables(&self) -> Result<String, String> {
        let slug = self.slug()?;
        LEVELS
            .iter()
            .map(|level| {
                self.colors
                    .get(level)
                    .map(|hex| format!("{slug}{level} = \"{hex}\""))
                    .ok_or("Missing shade".into())
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|v| v.join("\n"))
    }
    pub fn json(&self) -> Result<String, String> {
        // Serialize directly: a BTreeMap<u16,..> retains JS numeric property order.
        Ok(format!(
            "{{{}:{}}}",
            serde_json::to_string(self.slug()?).map_err(|e| e.to_string())?,
            serde_json::to_string(&self.colors).map_err(|e| e.to_string())?
        ))
    }
}
