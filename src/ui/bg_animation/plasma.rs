//! Classic analytic plasma effect.
//!
//! Each frame samples a sum-of-sines field on a `SUB × SUB` sub-pixel
//! grid inside every character cell, then averages the upper `SUB/2`
//! rows into the cell's top-half color and the lower `SUB/2` rows into
//! its bottom-half color. The cell is rendered as `▀` (upper half
//! block) with those two averages painted as the foreground and
//! background, so a single character carries the integrated plasma
//! value over its area instead of one point sample. The field's phase
//! advances by `speed` per frame, giving the colored bands their flow.

use std::time::{SystemTime, UNIX_EPOCH};

use tui::{
  buffer::Buffer,
  layout::{Position, Rect},
  style::Color,
};

use super::Animation;

/// Upper-half-block character used to split each terminal cell into two
/// vertically stacked color samples.
const UPPER_HALF: char = '▀';

/// Supersampling factor: each terminal cell averages `SUB × SUB`
/// plasma samples. Must be even so the top/bottom halves get equal
/// sub-pixel counts.
const SUB: usize = 2;

/// Configurable parameters for the plasma effect.
#[derive(Debug, Clone)]
pub struct Options {
  /// Spatial scale: larger values stretch the plasma bands. Clamped to
  /// `>= 1.0`.
  pub scale: f32,
  /// Phase advance per frame. Higher = faster motion. Clamped to
  /// `-1.0..=1.0`.
  pub speed: f32,
  /// Color at the low end of the gradient (plasma minimum).
  pub low:   Color,
  /// Color at the middle of the gradient (plasma midpoint).
  pub mid:   Color,
  /// Color at the high end of the gradient (plasma maximum).
  pub high:  Color,
}

impl Default for Options {
  fn default() -> Self {
    Self {
      scale: 18.0,
      speed: 0.06,
      low:   Color::Rgb(0x1A, 0x20, 0x80),
      mid:   Color::Rgb(0xA0, 0x40, 0xC0),
      high:  Color::Rgb(0xFF, 0xC0, 0xE0),
    }
  }
}

pub struct Plasma {
  width:  u16,
  height: u16,
  /// Per-cell `(top_color, bottom_color)` pairs in row-major order.
  cells:  Vec<(Color, Color)>,
  opts:   Options,
  phase:  f32,
}

impl Plasma {
  pub fn new(mut opts: Options) -> Self {
    opts.scale = opts.scale.max(1.0);
    opts.speed = opts.speed.clamp(-1.0, 1.0);

    // Use a wall-clock-derived initial phase so the animation doesn't start
    // from the same pattern every launch.
    let seed = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map(|d| d.as_millis() as u64)
      .unwrap_or(0);
    let phase = (seed % 10_000) as f32 * 0.001;

    Self {
      width: 0,
      height: 0,
      cells: Vec::new(),
      opts,
      phase,
    }
  }

  /// Sample the plasma field at sub-pixel `(px, py)` for the current
  /// phase. Returns a value normalized to `[0, 1]`.
  fn sample(&self, px: f32, py: f32) -> f32 {
    let s = self.opts.scale;
    let t = self.phase;
    let x = px / s;
    let y = py / s;
    let v = (x + t).sin()
      + (y + t * 0.8).sin()
      + ((x + y) * 0.5 + t * 1.3).sin()
      + ((x * x + y * y).sqrt() * 0.6 + t * 1.7).sin();
    (v + 4.0) / 8.0
  }

  /// Interpolate between the three gradient stops based on `t in [0, 1]`.
  fn color_at(&self, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
      lerp_color(self.opts.low, self.opts.mid, t * 2.0)
    } else {
      lerp_color(self.opts.mid, self.opts.high, (t - 0.5) * 2.0)
    }
  }
}

/// Best-effort RGB extraction for the gradient stops. Named/indexed
/// colors don't expose a stable RGB tuple here, so fall back to a
/// neutral mid-gray: the plasma still animates, just without the user's
/// chosen palette.
fn to_rgb(c: Color) -> (u8, u8, u8) {
  match c {
    Color::Rgb(r, g, b) => (r, g, b),
    _ => (0x80, 0x80, 0x80),
  }
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
  let (ar, ag, ab) = to_rgb(a);
  let (br, bg, bb) = to_rgb(b);
  let lerp = |x: u8, y: u8| -> u8 {
    (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8
  };
  Color::Rgb(lerp(ar, br), lerp(ag, bg), lerp(ab, bb))
}

impl Animation for Plasma {
  fn resize(&mut self, area: Rect) {
    if area.width == self.width
      && area.height == self.height
      && !self.cells.is_empty()
    {
      return;
    }
    self.width = area.width;
    self.height = area.height;
    let len = self.width as usize * self.height as usize;
    self.cells.resize(len, (Color::Reset, Color::Reset));
  }

  fn step(&mut self) {
    if self.width == 0 || self.height == 0 {
      return;
    }
    self.phase += self.opts.speed;
    let w = self.width as usize;
    // Step sizes that walk a `SUB × SUB` grid across a cell while
    // keeping the plasma-coord aspect ratio (1 x-unit per cell, 2
    // y-units per cell) the same as the old point-sample version.
    let dx = 1.0 / SUB as f32;
    let dy = 2.0 / SUB as f32;
    let inv_half = 1.0 / ((SUB * SUB / 2) as f32);
    for y in 0..self.height as usize {
      let cell_top_y = (2 * y) as f32;
      for x in 0..w {
        let cell_left_x = x as f32;
        let mut top_sum = 0.0;
        let mut bot_sum = 0.0;
        for sy in 0..SUB {
          let py = cell_top_y + sy as f32 * dy;
          for sx in 0..SUB {
            let px = cell_left_x + sx as f32 * dx;
            let v = self.sample(px, py);
            if sy < SUB / 2 {
              top_sum += v;
            } else {
              bot_sum += v;
            }
          }
        }
        self.cells[y * w + x] = (
          self.color_at(top_sum * inv_half),
          self.color_at(bot_sum * inv_half),
        );
      }
    }
  }

  fn render(&self, area: Rect, buf: &mut Buffer) {
    if self.width == 0 || self.height == 0 {
      return;
    }
    let w = self.width as usize;
    for ly in 0..self.height {
      for lx in 0..self.width {
        let (top, bot) = self.cells[ly as usize * w + lx as usize];
        let x = area.x + lx;
        let y = area.y + ly;
        if let Some(out) = buf.cell_mut(Position { x, y }) {
          out.set_char(UPPER_HALF);
          out.set_fg(top);
          out.set_bg(bot);
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn sample_is_normalized() {
    let p = Plasma::new(Options::default());
    for (px, py) in [(0.0, 0.0), (12.3, 4.7), (-30.0, 99.0), (200.0, 200.0)] {
      let v = p.sample(px, py);
      assert!(v >= 0.0 && v <= 1.0, "sample out of range: {v}");
    }
  }

  #[test]
  fn color_at_endpoints_match_stops() {
    let p = Plasma::new(Options::default());
    assert_eq!(p.color_at(0.0), p.opts.low);
    assert_eq!(p.color_at(0.5), p.opts.mid);
    assert_eq!(p.color_at(1.0), p.opts.high);
  }

  #[test]
  fn color_at_clamps_out_of_range() {
    let p = Plasma::new(Options::default());
    assert_eq!(p.color_at(-1.0), p.opts.low);
    assert_eq!(p.color_at(2.0), p.opts.high);
  }

  #[test]
  fn options_clamp_scale_and_speed() {
    let p = Plasma::new(Options {
      scale: 0.0,
      speed: 5.0,
      ..Options::default()
    });
    assert!(p.opts.scale >= 1.0);
    assert_eq!(p.opts.speed, 1.0);
  }

  #[test]
  fn resize_changes_buffer_shape() {
    let mut p = Plasma::new(Options::default());
    p.resize(Rect::new(0, 0, 8, 4));
    assert_eq!(p.cells.len(), 32);
    p.resize(Rect::new(0, 0, 16, 6));
    assert_eq!(p.cells.len(), 96);
  }

  #[test]
  fn step_paints_all_cells_with_rgb() {
    let mut p = Plasma::new(Options::default());
    p.resize(Rect::new(0, 0, 12, 6));
    p.step();
    for (top, bot) in &p.cells {
      assert!(matches!(top, Color::Rgb(_, _, _)));
      assert!(matches!(bot, Color::Rgb(_, _, _)));
    }
  }

  #[test]
  fn step_advances_phase() {
    let mut p = Plasma::new(Options::default());
    p.resize(Rect::new(0, 0, 4, 2));
    let before = p.phase;
    p.step();
    assert!((p.phase - before - p.opts.speed).abs() < 1e-6);
  }
}
