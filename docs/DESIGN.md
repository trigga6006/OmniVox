# OmniVox — Design Sheet

*Direction: **painterly accents on jet black**, matte. A pitch-black canvas with a
muted oil-paint / mosaic palette, lava-led, used precisely and sparingly. Soft
rounded corners. No glows. Live reference: `design-lab/index.html`. These tokens map
1:1 to a future Tailwind v4 `@theme`.*

---

## 1. Principles

1. **Jet-black canvas.** The page is pure `#000`. Surfaces lift only a few values off
   zero; hierarchy comes from hairline borders, not elevation.
2. **Matte, never glossy.** No colored glows, no neon halos, no glassy top-sheen
   highlights. Color is *pigment*, not light. Only soft **neutral** drop-shadows on
   true overlays (modal, floating pill).
3. **Painterly, precise, sparing.** A muted oil-paint palette placed like tesserae —
   lava leads; the rest appear only where they carry meaning (a role/state).
4. **Soft corners.** Comfier than sharp — rounded-rect controls, fully-round toggles /
   status pills. Customer-facing, not aggressive.

---

## 2. Color

### Surfaces — warm jet black
| Token | Hex | Use |
|---|---|---|
| `--surface-0` | `#000000` | page / nav rail |
| `--surface-1` | `#0b0a08` | cards, panels (barely lifted) |
| `--surface-2` | `#15110d` | inputs, raised |
| `--surface-3` | `#1f1913` | hover |
| `--surface-4` | `#2a2219` | lightest |

Lifted surfaces carry a **very slight brown warmth** (r>g>b, Cursor-like) — the page
stays pure `#000`; the warmth only appears as things lift off the canvas.

### Text — warm canvas white
| Token | Hex |
|---|---|
| `--text-primary` | `#efe9df` |
| `--text-secondary` | `#ada395` |
| `--text-muted` | `#756b60` |
| `--text-faint` | `#4a443b` |

### Hairlines
`--line rgba(240,232,222,.08)` · `--line-strong rgba(240,232,222,.14)` · `--line-faint rgba(240,232,222,.04)`

### Painterly accents → roles
The "mosaic." Lead with lava; the rest are bound to product states and used sparingly.
| Token | Hex | Role |
|---|---|---|
| `--lava` | `#f04e2e` | **primary · dictation** (the lead accent) |
| `--clay` | `#c76a4c` | recording · error |
| `--ochre` | `#d2a24e` | processing · warning |
| `--sage` | `#9ba97b` | success |
| `--teal` | `#5e948c` | command |
| `--plum` | `#a1768e` | structured mode |
| `--slate` | `#6e809b` | spare cool tessera |
| `--cream` | `#e9dcc6` | glint / brightest highlight |

Each accent has a `…-soft` companion (~14–15% alpha) for fills/tints (e.g. toggle-on
track, focus ring, badge background, active-nav background). Accents are applied
**solid** — no gradients in the UI. The only gradient is the **`mosaic`** sample
(`lava→clay→ochre→sage→teal→plum`), reserved for data-viz.

Data-viz ramp (heatmap): `surface-2 → lava(.22) → lava(.5) → clay(.82) → ochre`.

---

## 3. Typography

- **Sans / display:** `Geist` (Vercel). Headings 600, tight tracking (`-0.025em`).
- **Mono:** `Geist Mono` — timers, metadata, kbd, eyebrows, the ASCII field.
- **Eyebrows:** mono, 10px, uppercase, `0.26em` tracking, muted.
- Scale (≈1.25): 11 / 12.5 / 14 / 16 / 20 / 27 / 36 / 48. Tabular numerals for any counters.

---

## 4. Geometry & motion

- **Radii:** `--r-sm 9px` (inputs, kbd, nav tiles) · `--r 13px` (buttons, cards) ·
  `--r-lg 18px` (bezels) · `--r-full 999px` (toggles, slider thumb, badges, capture pill,
  record button).
- **Double-bezel** cards = outer hairline shell (`--r-lg`, padding 6) + inner core
  (`--r`), both flat fills (no sheen).
- **Easing:** `cubic-bezier(0.32,0.72,0,1)` for state/entry; springy
  `cubic-bezier(0.16,1,0.3,1)` for pop-ins. ~200ms. Buttons lift `translateY(-1px)` on
  hover, press `scale(.99)`. No glow transitions.
- **Shadows:** none on inline cards. Soft **neutral** drop only on overlays
  (modal `0 24px 60px -34px rgba(0,0,0,.7)`). Focus ring = `0 0 0 3px var(--lava-soft)`.

---

## 5. Signature elements

- **Capture pill** (overlay): fully-round bar. Recording state shows a **regular volume
  waveform** — solid lava mirrored bars (a normal audio meter), *not* ASCII/dither.
  States by border color: idle (neutral) · recording (clay) · processing (ochre) ·
  structured (plum) · command (teal).
- **Record button:** circle, `--surface-2` fill, lava hairline border; hover brightens
  border + `scale(1.03)`. No glow.
- **ASCII as mosaic:** the purchased Memselon `AsciiMatrixImage` is an *occasional*
  decorative flourish (hero field only), rendered in the painterly palette so it reads as
  a glyph mosaic. Not used for the pill. See `dev/oiwebsite/.../AsciiMatrix.README.md`.
- **Dither as an interactive click-pulse:** the base UI stays fully matte/normal. A
  full-window `pointer-events-none` overlay (`ClickPulse.tsx`) flickers a small,
  *contained* patch of electric static at the pointer on every click — a bright contact
  flash, a crackle of multicolored ordered-dither (lava body, ochre/clay cooling, cream
  + rare cool sparks) with a few short arc filaments, then a residue that runs through
  the spot and cools (~600ms). The patch is a noise-warped irregular blob — **never an
  expanding ring** — and the texture is true per-frame static, so it shimmers like a live
  charge. A `"ascii"` alternative renders the same envelope as scattered Geist-Mono glyphs;
  cycle dither → ascii → off live with **Ctrl/Cmd+Alt+P** (persisted). Never baked into
  controls or the waveform; transient and interaction-driven only. Live A/B reference:
  `design-lab/click-pulse-lab.html`.

---

## 6. Notes for the port

- The lab keeps **legacy role aliases** (`--rose`→clay, `--violet`→plum, `--fuchsia`→teal,
  `--mint`→sage, `--amber`→ochre, `--cyan`→lava) so component CSS recolors in one place.
  When porting to OmniVox, rename usages to the canonical painterly tokens and drop the
  aliases.
- Build shared primitives (Button / Input / Toggle / Card / Badge / Pill) from these
  tokens **before** re-skinning pages — OmniVox currently has no component library.
