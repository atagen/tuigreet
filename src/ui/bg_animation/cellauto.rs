//! Elementary cellular automaton background, rendered through the
//! [`braille`](super::braille) subpixel canvas.
//!
//! One row of `2 * cells_w` binary cells evolves under a Wolfram rule
//! (0..=255). Each step the next generation is computed from the current
//! row using the 3-cell neighborhood `(left, self, right)` indexing into
//! the rule's bitmask, the canvas scrolls up by one subpixel row, and the
//! new generation lands at the bottom. Boundaries wrap.
//!
//! Inspired by FFmpeg's `lavfi` `cellauto` filter — same algorithm, same
//! rule numbering, just rendered into a 2x3-subpixel braille grid instead
//! of pixels.

use std::time::{SystemTime, UNIX_EPOCH};

use rand::{RngExt, SeedableRng, prelude::StdRng};
use tui::{buffer::Buffer, layout::Rect, style::Color};

use super::{Animation, braille::Braille};

/// How the CA's first generation is seeded.
#[derive(Debug, Clone)]
pub enum Init {
  /// One live cell dead-center on the bottom row. Combined with rules like
  /// 90 this produces the iconic Sierpiński fractal as the pattern scrolls
  /// upward.
  Single,
  /// Each cell on the bottom row is independently live with probability
  /// `density`. Produces chaotic-looking fills under most rules.
  Random {
    density: f32,
  },
}

impl Init {
  /// Parse a config string. Accepted forms:
  /// * `"single"` — [`Init::Single`]
  /// * `"random"` — [`Init::Random`] with default 0.5 density
  /// * `"random:0.3"` — [`Init::Random`] with explicit density 0..=1
  pub fn from_name(name: &str) -> Option<Self> {
    let trimmed = name.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower == "single" {
      return Some(Self::Single);
    }
    if lower == "random" {
      return Some(Self::Random { density: 0.5 });
    }
    if let Some(rest) = lower.strip_prefix("random:")
      && let Ok(d) = rest.parse::<f32>()
    {
      return Some(Self::Random { density: d.clamp(0.0, 1.0) });
    }
    None
  }
}

/// Configurable parameters.
#[derive(Debug, Clone)]
pub struct Options {
  /// Wolfram rule number, 0..=255. 110 is Turing-complete and visually
  /// dense; 90 from a single seed gives Sierpiński; 30 looks like noise.
  pub rule: u8,

  /// Initial seed strategy.
  pub init: Init,

  /// Foreground color used for every lit cell (the CA is monochrome,
  /// matching lavfi's behavior).
  pub color: Color,
}

impl Default for Options {
  fn default() -> Self {
    Self {
      rule:  110,
      init:  Init::Single,
      color: Color::Rgb(0x55, 0xCC, 0xFF),
    }
  }
}

pub struct Cellauto {
  canvas:  Braille,
  opts:    Options,
  rng:     StdRng,
  // Scratch buffers for the 1D evolution. Kept across steps so we don't
  // reallocate every frame.
  current: Vec<bool>,
  next:    Vec<bool>,
  // True until `step` is first called after a `resize`. Lets us defer
  // seeding to step 0 so the first frame is the seed and the second frame
  // is the first generation.
  needs_seed: bool,
}

impl Cellauto {
  pub fn new(opts: Options) -> Self {
    let seed = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map(|d| d.as_nanos() as u64)
      .unwrap_or(0);
    Self {
      canvas:     Braille::new(Rect::default()),
      opts,
      rng:        StdRng::seed_from_u64(seed),
      current:    Vec::new(),
      next:       Vec::new(),
      needs_seed: true,
    }
  }

  /// Apply the configured initial condition to the bottom row of the
  /// canvas. Called from `step` whenever `needs_seed` is set, which
  /// happens after construction and after every `resize`.
  fn seed(&mut self) {
    let w = self.canvas.width();
    if w == 0 {
      return;
    }
    self.canvas.clear();
    self.current.clear();
    self.current.resize(w, false);
    match self.opts.init {
      Init::Single => {
        self.current[w / 2] = true;
      },
      Init::Random { density } => {
        for cell in self.current.iter_mut() {
          *cell = self.rng.random_bool(density.clamp(0.0, 1.0) as f64);
        }
      },
    }
    self.canvas.write_bottom_row(&self.current);
    self.next.clear();
    self.next.resize(w, false);
    self.needs_seed = false;
  }
}

impl Animation for Cellauto {
  fn resize(&mut self, area: Rect) {
    let (old_w, old_h) = self.canvas.cells();
    self.canvas.resize(area);
    if (old_w, old_h) != self.canvas.cells() {
      // Geometry changed (or this is the first resize) — re-seed on the
      // next step so the initial condition applies to the new width.
      self.needs_seed = true;
    }
  }

  fn step(&mut self) {
    if self.needs_seed {
      self.seed();
      return;
    }
    let w = self.canvas.width();
    if w == 0 {
      return;
    }

    // current already holds the previous generation from the last step
    // (or from seed). Compute next from current under the configured
    // rule, with wrap boundaries.
    let rule = self.opts.rule;
    for x in 0..w {
      let l = self.current[(x + w - 1) % w] as u8;
      let c = self.current[x] as u8;
      let r = self.current[(x + 1) % w] as u8;
      let bit = (l << 2) | (c << 1) | r;
      self.next[x] = ((rule >> bit) & 1) == 1;
    }

    self.canvas.scroll_up();
    self.canvas.write_bottom_row(&self.next);
    std::mem::swap(&mut self.current, &mut self.next);
  }

  fn render(&self, area: Rect, buf: &mut Buffer) {
    self.canvas.render(area, buf, self.opts.color);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn fixed(opts: Options) -> Cellauto {
    let mut c = Cellauto::new(opts);
    c.rng = StdRng::seed_from_u64(7);
    c
  }

  #[test]
  fn init_from_name_parses_modes() {
    assert!(matches!(Init::from_name("single"), Some(Init::Single)));
    assert!(matches!(
      Init::from_name("random"),
      Some(Init::Random { density }) if (density - 0.5).abs() < 1e-6
    ));
    assert!(matches!(
      Init::from_name("random:0.25"),
      Some(Init::Random { density }) if (density - 0.25).abs() < 1e-6
    ));
    assert!(Init::from_name("garbage").is_none());
    assert!(Init::from_name("random:nope").is_none());
  }

  #[test]
  fn rule_zero_kills_everything() {
    let mut c = fixed(Options {
      rule: 0,
      init: Init::Random { density: 0.8 },
      ..Options::default()
    });
    c.resize(Rect::new(0, 0, 8, 4)); // 16x12 subpixels
    c.step(); // seeds
    c.step(); // first real generation under rule 0
    let (w, h) = c.canvas.dims();
    let any_alive = (0..w).any(|x| c.canvas.get(x, h - 1));
    assert!(!any_alive, "rule 0 must produce all-dead bottom row");
  }

  #[test]
  fn rule_255_revives_everything() {
    let mut c = fixed(Options {
      rule: 255,
      init: Init::Single,
      ..Options::default()
    });
    c.resize(Rect::new(0, 0, 8, 4));
    c.step(); // seeds: one bit on bottom row
    c.step(); // rule 255 -> every cell becomes 1
    let (w, h) = c.canvas.dims();
    let all_alive = (0..w).all(|x| c.canvas.get(x, h - 1));
    assert!(all_alive, "rule 255 must produce all-alive bottom row");
  }

  #[test]
  fn rule_90_single_seed_produces_sierpinski_apex() {
    // Rule 90 (XOR of left and right neighbors) from a single bit forms
    // the Sierpiński triangle. After N generations the live cells on the
    // current row are exactly those at Pascal's-triangle row N positions
    // where the binomial coefficient is odd. We verify the first three
    // generations directly: positions {±0}, {±1}, {±2, 0}.
    let mut c = fixed(Options {
      rule: 90,
      init: Init::Single,
      ..Options::default()
    });
    c.resize(Rect::new(0, 0, 16, 4)); // 32x12 subpixels, plenty wide
    let (w, h) = c.canvas.dims();
    let mid = w / 2;

    c.step(); // seeds: only mid is alive on the bottom row
    assert!(c.canvas.get(mid, h - 1));
    assert!(!c.canvas.get(mid - 1, h - 1));
    assert!(!c.canvas.get(mid + 1, h - 1));

    c.step(); // gen 1: mid-1 and mid+1 alive, mid dead
    assert!(c.canvas.get(mid - 1, h - 1));
    assert!(c.canvas.get(mid + 1, h - 1));
    assert!(!c.canvas.get(mid, h - 1));

    c.step(); // gen 2: mid-2 and mid+2 alive (Pascal row 2 is 1,2,1 — the
              // center binomial is even, so the middle cell dies)
    assert!(c.canvas.get(mid - 2, h - 1));
    assert!(c.canvas.get(mid + 2, h - 1));
    assert!(!c.canvas.get(mid - 1, h - 1));
    assert!(!c.canvas.get(mid, h - 1));
    assert!(!c.canvas.get(mid + 1, h - 1));
  }

  #[test]
  fn boundaries_wrap() {
    // Rule 90 with a single seed at x=0 should after one step light
    // positions x=W-1 and x=1 (its wrap-around neighbors).
    let mut c = fixed(Options {
      rule: 90,
      init: Init::Single,
      ..Options::default()
    });
    c.resize(Rect::new(0, 0, 4, 2)); // 8x6 subpixels
    let (w, h) = c.canvas.dims();
    // Manually seed at x=0 instead of the default mid.
    c.step(); // initial seed at mid (w/2); we override below
    c.canvas.clear();
    c.current.fill(false);
    c.current[0] = true;
    c.canvas.write_bottom_row(&c.current);

    c.step();
    assert!(c.canvas.get(w - 1, h - 1), "wrap from x=0 must light x=W-1");
    assert!(c.canvas.get(1, h - 1), "neighbor x=1 must be lit");
    assert!(!c.canvas.get(0, h - 1), "rule 90 kills the seed cell");
  }

  #[test]
  fn resize_reseeds() {
    let mut c = fixed(Options::default());
    c.resize(Rect::new(0, 0, 4, 2));
    c.step(); // seed
    c.step(); // one generation
    c.resize(Rect::new(0, 0, 8, 3));
    assert!(c.needs_seed);
    c.step(); // re-seeds at new width
    let (w, h) = c.canvas.dims();
    let alive = (0..w).filter(|&x| c.canvas.get(x, h - 1)).count();
    // Single-seed default: exactly one live cell.
    assert_eq!(alive, 1);
  }
}
