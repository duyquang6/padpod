// Copyright 2026 ligt (https://github.com/duyquang6/padpod)
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Scalable text. Glyphs are rasterised once per (character, size) and cached.
//!
//! Album names mix Latin, Vietnamese, Korean and Chinese, so the first choice
//! is spruce's theme font (Nunito merged with a CJK face). The embedded Nunito
//! subset is the fallback for a device where that file is missing.

use crate::canvas::{Canvas, Rgb};
use std::collections::HashMap;

const EMBEDDED: &[u8] = include_bytes!("../assets/Nunito-Regular-subset.ttf");

struct Glyph {
    metrics: fontdue::Metrics,
    coverage: Vec<u8>,
}

pub struct Fonts {
    /// Tried in order; a character is drawn with the first face that has it.
    faces: Vec<fontdue::Font>,
    cache: HashMap<(char, u32), Glyph>,
}

impl Fonts {
    pub fn load(paths: &[String]) -> Self {
        let mut faces = Vec::new();
        for path in paths {
            let parsed = std::fs::read(path)
                .map_err(|e| e.to_string())
                .and_then(|bytes| parse(&bytes).map_err(str::to_string));
            match parsed {
                Ok(face) => {
                    eprintln!("font: {path}");
                    faces.push(face);
                    break;
                }
                Err(e) => eprintln!("font: {path} not usable ({e})"),
            }
        }
        if faces.is_empty() && !paths.is_empty() {
            eprintln!("font: embedded Nunito only - CJK names will show as boxes");
        }
        faces.push(parse(EMBEDDED).expect("the embedded font parses"));
        Self { faces, cache: HashMap::new() }
    }

    fn glyph(&mut self, ch: char, size: f32) -> &Glyph {
        let faces = &self.faces;
        self.cache.entry((ch, size.to_bits())).or_insert_with(|| {
            let face = faces.iter().find(|f| f.lookup_glyph_index(ch) != 0).unwrap_or(&faces[0]);
            let (metrics, coverage) = face.rasterize(ch, size);
            Glyph { metrics, coverage }
        })
    }

    pub fn measure(&mut self, text: &str, size: f32) -> i32 {
        text.chars().map(|ch| self.glyph(ch, size).metrics.advance_width).sum::<f32>().round() as i32
    }

    /// Draw `text` with its baseline at `baseline_y`; returns the x just past it.
    pub fn draw(&mut self, canvas: &mut Canvas, text: &str, x: i32, baseline_y: i32, size: f32, colour: Rgb) -> i32 {
        let mut pen = x as f32;
        for ch in text.chars() {
            let g = self.glyph(ch, size);
            let left = pen.round() as i32 + g.metrics.xmin;
            let top = baseline_y - g.metrics.ymin - g.metrics.height as i32;
            canvas.blend_mask(left, top, g.metrics.width, &g.coverage, colour);
            pen += g.metrics.advance_width;
        }
        pen.round() as i32
    }

    pub fn draw_centred(&mut self, canvas: &mut Canvas, text: &str, centre_x: i32, baseline_y: i32, size: f32, colour: Rgb) {
        let w = self.measure(text, size);
        self.draw(canvas, text, centre_x - w / 2, baseline_y, size, colour);
    }

    /// `text` cut to fit `max_w`, ending in an ellipsis when something was cut.
    pub fn ellipsize(&mut self, text: &str, size: f32, max_w: i32) -> String {
        if self.measure(text, size) <= max_w {
            return text.to_string();
        }
        let budget = (max_w - self.measure("…", size)) as f32;
        let mut out = String::new();
        let mut w = 0.0;
        for ch in text.chars() {
            w += self.glyph(ch, size).metrics.advance_width;
            if w > budget {
                break;
            }
            out.push(ch);
        }
        out.truncate(out.trim_end().len());
        out.push('…');
        out
    }

    /// Break `text` at spaces into at most `max_lines` lines of `max_w`; the
    /// last line is ellipsized if the text still does not fit.
    pub fn wrap(&mut self, text: &str, size: f32, max_w: i32, max_lines: usize) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut words = text.split(' ').peekable();
        while let Some(word) = words.next() {
            let candidate = if current.is_empty() { word.to_string() } else { format!("{current} {word}") };
            if current.is_empty() || self.measure(&candidate, size) <= max_w {
                current = candidate;
                continue;
            }
            if lines.len() + 1 == max_lines {
                // No room for another line: everything left goes on this one.
                let rest: Vec<&str> = std::iter::once(word).chain(words).collect();
                current = format!("{current} {}", rest.join(" "));
                break;
            }
            lines.push(std::mem::replace(&mut current, word.to_string()));
        }
        lines.push(self.ellipsize(&current, size, max_w));
        lines
    }
}

fn parse(bytes: &[u8]) -> Result<fontdue::Font, &'static str> {
    fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_font_file_falls_back_to_the_embedded_face() {
        let mut fonts = Fonts::load(&["/nonexistent/font.ttf".to_string()]);
        assert!(fonts.measure("Tải ảnh", 30.0) > 0);
    }

    #[test]
    fn short_text_is_left_alone_and_long_text_gets_an_ellipsis() {
        let mut fonts = Fonts::load(&[]);
        assert_eq!(fonts.ellipsize("abc", 30.0, 1000), "abc");
        let cut = fonts.ellipsize("a rather long album title indeed", 30.0, 150);
        assert!(cut.ends_with('…'));
        assert!(fonts.measure(&cut, 30.0) <= 150);
    }

    #[test]
    fn wrapping_respects_the_line_limit_and_the_width() {
        let mut fonts = Fonts::load(&[]);
        let text = "one two three four five six seven eight nine ten eleven twelve";
        let lines = fonts.wrap(text, 30.0, 200, 2);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].ends_with('…'));
        for line in &lines {
            assert!(fonts.measure(line, 30.0) <= 200, "{line}");
        }
        assert_eq!(fonts.wrap("short", 30.0, 200, 2), vec!["short"]);
    }
}
