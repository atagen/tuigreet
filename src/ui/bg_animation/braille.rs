//! Subpixel canvas rendered as Unicode braille.
//!
//! Each terminal cell holds a `2 × 3` grid of subpixels, encoded as one
//! braille glyph in `U+2800..=U+283F` (the lower 6 of braille's 8 dots).
//! Animations work in the higher-resolution subpixel space — `set(sx, sy,
//! true)` to light a dot, then [`Braille::render`] paints the whole canvas
//! to a ratatui [`Buffer`] in one pass.
//!
//! This module is intentionally generic: it knows nothing about the
//! animation that drives it. It is reused by 1D and 2D effects that want
//! to render high-resolution patterns inside a terminal cell grid.

use tui::{
  buffer::Buffer,
  layout::{Position, Rect},
  style::Color,
};

/// Subpixel columns per terminal cell.
pub const SUBPIXELS_X: usize = 2;
/// Subpixel rows per terminal cell. Lower 6 dots of an 8-dot braille glyph.
pub const SUBPIXELS_Y: usize = 3;
/// First braille glyph (`U+2800` = `⠀`, all dots off).
pub const BRAILLE_BASE: u32 = 0x2800;

/// Bit positions of each subpixel within a braille glyph mask. Indexed as
/// `BRAILLE_BIT[sx_in_cell][sy_in_cell]`. Matches the Unicode braille
/// pattern layout for dots 1-6.
const BRAILLE_BIT: [[u8; SUBPIXELS_Y]; SUBPIXELS_X] = [[0, 1, 2], [3, 4, 5]];

/// A canvas of subpixels backed by a flat `Vec<bool>` in row-major order.
/// Dimensions are derived from the terminal area: `width = 2 * cells_w`,
/// `height = 3 * cells_h`.
pub struct Braille {
  cells_w: u16,
  cells_h: u16,
  bits:    Vec<bool>,
}

impl Braille {
  /// Create an empty canvas sized for `area`
  pub fn new(area: Rect) -> Self {
    let mut b = Self {
      cells_w: 0,
      cells_h: 0,
      bits:    Vec::new(),
    };
    b.resize(area);
    b
  }

  /// React to a (possibly new) terminal size
  pub fn resize(&mut self, area: Rect) {
    if area.width == self.cells_w && area.height == self.cells_h {
      return;
    }
    self.cells_w = area.width;
    self.cells_h = area.height;
    self.bits.clear();
    self.bits.resize(self.width() * self.height(), false);
  }

  pub fn clear(&mut self) {
    self.bits.fill(false);
  }

  pub fn dims(&self) -> (usize, usize) {
    (self.width(), self.height())
  }

  /// Subpixel width: `2 * cells_w`
  pub fn width(&self) -> usize {
    self.cells_w as usize * SUBPIXELS_X
  }

  /// Subpixel height: `3 * cells_h`
  pub fn height(&self) -> usize {
    self.cells_h as usize * SUBPIXELS_Y
  }

  /// Terminal cell dimensions
  pub fn cells(&self) -> (u16, u16) {
    (self.cells_w, self.cells_h)
  }

  fn idx(&self, sx: usize, sy: usize) -> Option<usize> {
    let w = self.width();
    let h = self.height();
    if sx >= w || sy >= h {
      return None;
    }
    Some(sy * w + sx)
  }

  /// Light or extinguish a single subpixel.
  /// Out-of-bounds writes are a no-op, so callers can paint patterns that don't
  /// fit the canvas without bounds-checking
  pub fn set(&mut self, sx: usize, sy: usize, on: bool) {
    if let Some(i) = self.idx(sx, sy) {
      self.bits[i] = on;
    }
  }

  pub fn get(&self, sx: usize, sy: usize) -> bool {
    self.idx(sx, sy).map_or(false, |i| self.bits[i])
  }

  /// Shift every subpixel row up by one. The top row is dropped and the
  /// new bottom row is left zeroed for the caller to fill.
  pub fn scroll_up(&mut self) {
    let w = self.width();
    let h = self.height();
    if w == 0 || h < 2 {
      if !self.bits.is_empty() {
        self.bits.fill(false);
      }
      return;
    }
    self.bits.copy_within(w..w * h, 0);
    self.bits[w * (h - 1)..].fill(false);
  }

  /// Copy the bottom subpixel row into `dst`. Panics if `dst.len() <
  /// self.width()`.
  pub fn read_bottom_row(&self, dst: &mut [bool]) {
    let w = self.width();
    let h = self.height();
    if w == 0 || h == 0 {
      return;
    }
    let start = (h - 1) * w;
    dst[..w].copy_from_slice(&self.bits[start..start + w]);
  }

  /// Overwrite the bottom subpixel row from `src`. Panics if `src.len() <
  /// self.width()`.
  pub fn write_bottom_row(&mut self, src: &[bool]) {
    let w = self.width();
    let h = self.height();
    if w == 0 || h == 0 {
      return;
    }
    let start = (h - 1) * w;
    self.bits[start..start + w].copy_from_slice(&src[..w]);
  }

  /// Compute the braille glyph mask for the terminal cell at `(cx, cy)`.
  /// Returns `0` when the cell is entirely empty so callers can skip it.
  fn cell_mask(&self, cx: u16, cy: u16) -> u8 {
    let mut mask = 0u8;
    let base_sx = cx as usize * SUBPIXELS_X;
    let base_sy = cy as usize * SUBPIXELS_Y;
    for dx in 0..SUBPIXELS_X {
      for dy in 0..SUBPIXELS_Y {
        if self.get(base_sx + dx, base_sy + dy) {
          mask |= 1 << BRAILLE_BIT[dx][dy];
        }
      }
    }
    mask
  }

  /// Paint the canvas to `buf` using a single foreground color. Cells with
  /// no lit subpixels are skipped — the underlying buffer cell shows
  /// through.
  pub fn render(&self, area: Rect, buf: &mut Buffer, fg: Color) {
    self.render_with(area, buf, |_, _| fg);
  }

  /// Like [`Braille::render`] but the foreground color comes from a
  /// closure `(cx, cy) -> Color` keyed on the terminal cell coordinates.
  /// Useful for ageing/gradient effects.
  pub fn render_with<F>(&self, area: Rect, buf: &mut Buffer, color: F)
  where
    F: Fn(u16, u16) -> Color,
  {
    if self.cells_w == 0 || self.cells_h == 0 {
      return;
    }
    for cy in 0..self.cells_h {
      for cx in 0..self.cells_w {
        let mask = self.cell_mask(cx, cy);
        if mask == 0 {
          continue;
        }
        let glyph = char::from_u32(BRAILLE_BASE + mask as u32).unwrap_or(' ');
        let x = area.x + cx;
        let y = area.y + cy;
        if let Some(cell) = buf.cell_mut(Position { x, y }) {
          cell.set_char(glyph);
          cell.set_fg(color(cx, cy));
          cell.set_bg(Color::Reset);
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn dims_match_terminal_area() {
    let b = Braille::new(Rect::new(0, 0, 8, 4));
    assert_eq!(b.cells(), (8, 4));
    assert_eq!(b.width(), 16);
    assert_eq!(b.height(), 12);
  }

  #[test]
  fn set_get_roundtrip() {
    let mut b = Braille::new(Rect::new(0, 0, 4, 2));
    b.set(3, 5, true);
    assert!(b.get(3, 5));
    assert!(!b.get(0, 0));
  }

  #[test]
  fn out_of_bounds_set_is_noop() {
    let mut b = Braille::new(Rect::new(0, 0, 2, 1));
    b.set(99, 99, true);
    assert!(!b.get(99, 99));
  }

  #[test]
  fn cell_mask_matches_braille_layout() {
    // Light all six dots in the top-left cell — should produce U+283F (⠿).
    let mut b = Braille::new(Rect::new(0, 0, 2, 1));
    for dx in 0..2 {
      for dy in 0..3 {
        b.set(dx, dy, true);
      }
    }
    assert_eq!(b.cell_mask(0, 0), 0b0011_1111);
    let glyph =
      char::from_u32(BRAILLE_BASE + b.cell_mask(0, 0) as u32).unwrap();
    assert_eq!(glyph, '⠿');
  }

  #[test]
  fn cell_mask_individual_dot_positions() {
    // Verify each subpixel maps to the documented braille bit. Spec dots
    // 1-6 correspond to (dx, dy) -> bit index BRAILLE_BIT[dx][dy].
    let cases = [
      ((0, 0), 0b0000_0001, '⠁'), // dot 1
      ((0, 1), 0b0000_0010, '⠂'), // dot 2
      ((0, 2), 0b0000_0100, '⠄'), // dot 3
      ((1, 0), 0b0000_1000, '⠈'), // dot 4
      ((1, 1), 0b0001_0000, '⠐'), // dot 5
      ((1, 2), 0b0010_0000, '⠠'), // dot 6
    ];
    for ((dx, dy), expect_mask, expect_glyph) in cases {
      let mut b = Braille::new(Rect::new(0, 0, 1, 1));
      b.set(dx, dy, true);
      assert_eq!(b.cell_mask(0, 0), expect_mask, "subpixel ({dx},{dy})");
      let g = char::from_u32(BRAILLE_BASE + expect_mask as u32).unwrap();
      assert_eq!(g, expect_glyph);
    }
  }

  #[test]
  fn scroll_up_drops_top_zeros_bottom() {
    let mut b = Braille::new(Rect::new(0, 0, 1, 2)); // 2x6 subpixels
    // Set distinctive marks on each row.
    for sx in 0..2 {
      b.set(sx, 0, true); // top row
      b.set(sx, 5, true); // bottom row
    }
    b.scroll_up();
    // After scroll: what was sy=1 is now sy=0; the old top (sy=0) is gone.
    // Old sy=5 is now sy=4. New sy=5 is zeroed.
    assert!(!b.get(0, 0));
    assert!(b.get(0, 4));
    assert!(b.get(1, 4));
    assert!(!b.get(0, 5));
    assert!(!b.get(1, 5));
  }

  #[test]
  fn bottom_row_read_write_roundtrip() {
    let mut b = Braille::new(Rect::new(0, 0, 4, 2)); // width=8, height=6
    let src = [true, false, true, true, false, false, true, false];
    b.write_bottom_row(&src);
    let mut dst = [false; 8];
    b.read_bottom_row(&mut dst);
    assert_eq!(dst, src);
  }

  #[test]
  fn render_writes_braille_glyphs_for_lit_cells() {
    let mut b = Braille::new(Rect::new(0, 0, 3, 1));
    // Light only the top-left dot of cell (0,0).
    b.set(0, 0, true);
    let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
    b.render(Rect::new(0, 0, 3, 1), &mut buf, Color::White);
    assert_eq!(buf[(0, 0)].symbol(), "⠁");
    // Empty cells stay at their default (single space).
    assert_eq!(buf[(1, 0)].symbol(), " ");
    assert_eq!(buf[(2, 0)].symbol(), " ");
  }

  #[test]
  fn render_with_uses_per_cell_color() {
    let mut b = Braille::new(Rect::new(0, 0, 2, 1));
    b.set(0, 0, true);
    b.set(2, 0, true);
    let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
    b.render_with(Rect::new(0, 0, 2, 1), &mut buf, |cx, _| {
      if cx == 0 { Color::Red } else { Color::Blue }
    });
    assert_eq!(buf[(0, 0)].fg, Color::Red);
    assert_eq!(buf[(1, 0)].fg, Color::Blue);
  }
}
