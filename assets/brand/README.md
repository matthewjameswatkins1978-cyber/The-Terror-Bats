# Terror Bats brand assets (RC1)

- Canonical name: **The Terror Bats Framework**
- Short name: **Terror Bats**
- Technical name: **terrorbats** (binary, crate)
- Sigil: `\^v^/` — terminal punctuation, never ASCII theatre
- Mark: `terror-bats-mark.svg` — angular bat from the `^v^` geometry:
  scalloped wings, twin-ear peaks, downward evidence-point body

## Files

- `terror-bats-mark.svg` — canonical source (single-colour `#1a1a1a`)
- `terror-bats-icon-{512,256,128,64,32,16}.png` — Inkscape renders
- `terrorbats.ico` — multi-image Windows icon (16–512)

## Rules

- Monochrome first: the mark must read in pure black on white and pure
  white on black. Colour is never load-bearing.
- Legible at 16×16: silhouette only, no fine detail.
- Dark and light backgrounds: use the solid mark; do not add outlines
  or glows to force contrast — pick the ink colour per background.
- Spacing: clear space at least the body height on all sides.
- Do not: Halloween clip-art, cute mascots, goth-metal lettering,
  skulls, shields, gradients, or font files (system fonts only).

## Provenance

PNGs are Inkscape renders of the committed SVG; the ICO bundles those
renders via ImageMagick. `tests/brand_assets.rs` verifies file presence,
ICO structure, and PNG dimensions. The Windows executable embeds
`terrorbats.ico` at link time via `winres` (see `build.rs`).
