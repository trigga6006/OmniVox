# OmniVox — UI/UX Surface Map

*Canonical map of the current product, compiled ahead of the visual overhaul. Token
values, file paths, and component names are cited throughout so this can serve as a
single source of truth. Compiled 2026-06-22 from a full read-only survey of every
page, overlay, and shared system.*

---

## 1. What OmniVox Is

OmniVox is a **local-first desktop dictation app** (Tauri + React 19 + Tailwind v4)
that turns speech into text anywhere on the system via a global push-to-talk hotkey,
with optional LLM "Structured Mode" post-processing and a voice "Command Mode" for
launching apps. It runs as a **two-window architecture**:

- **Main window** (`index.html` / `src/main.tsx`) — the full app behind a 68px left
  navigation rail. IA = 8 lazy-loaded pages routed off `appStore.currentPage`:
  Dictation, History, Notes, Dictionary, Context Modes, Models, Analytics, Settings
  (Settings pinned below a separator).
- **Frameless, always-on-top overlay window** (`overlay.html` / `src/overlay-main.tsx`)
  — a tiny floating pill rendered over any application for live capture, with a
  constellation of sub-surfaces (waveforms, mode selector, control column, structured
  panel, command pill, popups, degraded banner).

Two shared systems cut across everything: the toast stack and a global ErrorBoundary,
both fed by a Tauri event/command bridge (`src/lib/tauri.ts`).

---

## 2. Surface Inventory

| Surface | Type | Purpose |
|---|---|---|
| **Dictation Page** (`features/dictation/DictationPanel.tsx`) | Main page | Primary voice-to-text flow: 96px record button, audio visualizer, stats, last-transcription card |
| **History Page** (`features/history/HistoryPage.tsx`) | Main page | Searchable, paginated archive of past transcriptions with per-item copy/delete |
| **Notes Page** (`features/notes/NotesPage.tsx`) | Main page | Dual-mode (card grid + fullscreen editor) note manager with debounced auto-save |
| **Dictionary Page** (`features/dictionary/DictionaryPage.tsx`) | Main page | Three tabs — Vocabulary, Words (replacements), Snippets (expansions) — inline CRUD |
| **Context Modes Page** (`features/modes/ContextModesPage.tsx`) | Main page | Writing-style profiles + nested dictionary/snippet/app-binding management |
| **Models Page** (`features/models/ModelsPage.tsx`) | Main page | Two tabs — Speech (Whisper, amber) and LLM Structuring (violet) — download/activate + config |
| **Analytics Page** (`features/analytics/UserAnalyticsPage.tsx`) | Main page | Usage dashboard: ledger stats, 52-week heatmap, peak-hours + 30-day bar charts |
| **Settings Page** (`features/settings/SettingsPage.tsx`) | Main page | Masonry of config cards: hotkey, GPU, output, audio, behavior, appearance, about |
| **Sidebar** (`app/Sidebar.tsx`) | Shared shell | 68px nav rail with logo, page buttons, settings pin, live recording dot |
| **Floating Pill** (`features/overlay/FloatingPill.tsx`) | Overlay root | Always-on-top capture widget; routes all overlay states & sub-surfaces |
| **Idle / Active Waveforms** (`IdleWaveform.tsx`, `PillWaveform.tsx`) | Overlay | 5-bar ambient (idle) and 12-bar audio-driven (recording) visualizers |
| **Mode Selector** (`ModeSelector.tsx`) | Overlay | Dropdown of active context modes + "Open OmniVox" footer |
| **Ley Line Toggle** (`StructuredModeToggle.tsx`) | Overlay | 28×64 vertical capsule toggling Structured Mode (amber off / violet on) |
| **Quick-Toggles & Ghost** (`FloatingPill.css`) | Overlay | 26px circle toggles: auto-switch, live-preview, noise-reduction, ship, ghost |
| **Structured Panel** (`StructuredPanel.tsx`) | Overlay | 420×480 LLM-output preview/edit/paste panel with reverse-Dynamic-Island entrance |
| **Command Pill** (`CommandPill.tsx`) | Overlay | Indigo-accented voice-command capture/confirm UI (mutually exclusive w/ dictation) |
| **Ship / Ley-Line Popups** (`FloatingPill.css`) | Overlay | Right-click popovers gating Command-Send and Voxify voice triggers |
| **Degraded Banner** (`FloatingPill.tsx`) | Overlay | Transient warning on LLM timeout / GPU fallback; auto-dismiss 15–20s |
| **Toast System** (`ToastContainer.tsx`, `toastStore.ts`) | Shared | Bottom-right error/warn/info notifications with dedup + optional actions |
| **Error Boundary** (`ErrorBoundary.tsx`) | Shared | Full-screen render-error recovery with "Reload UI" |
| **Design Tokens / Theming** (`styles/tokens.css`, `styles/globals.css`, `app/providers.tsx`) | Shared system | OKLCH color scales, type scale, animations, shadows; dark/light theme sync |

---

## 3. Current Design Language — "Soft Brown / Yellow"

The aesthetic reads **soft, warm, brown-honey** because of a deliberate, restrained
system rather than any single bold choice. Everything lives in **OKLCH** with a
consistent warm hue and very low chroma; the brand pops only through a desaturated
amber accent. Master tokens: `src/styles/tokens.css` (`@theme` = dark defaults;
`[data-theme="light"]` overrides).

### Warm-low-chroma OKLCH approach
Nearly every neutral sits at **hue ~70** (warm yellow-brown cast) with **ultra-low
chroma (0.004–0.009)**. This is the engine of the "brown" feeling: surfaces are not
gray, they are *warm* gray — near-neutral with a whisper of brown that reads as soft
and paper-like even in the dark.

**Dark surfaces:** `surface-0` `oklch(0.175 0.005 70)` (page bg) → `surface-1`
`oklch(0.218 0.006 70)` (cards) → `surface-2` `oklch(0.258 0.007 70)` (inputs) →
`surface-3` `oklch(0.305 0.008 70)` (hover) → `surface-4` `oklch(0.360 0.009 70)`.

**Dark text:** `text-primary` `oklch(0.970 0.004 70)` (~18:1 → AAA), `text-secondary`
`oklch(0.785 0.006 70)`, `text-muted` `oklch(0.580 0.008 70)`.

**Borders:** `--color-border` `oklch(0.305 0.008 70)` — *identical to surface-3*, so
borders are intentionally near-invisible. Hover `0.395`, active `oklch(0.555 0.080 65)`
picks up amber chroma.

### Amber / honey accent scale
A **desaturated honey**, never a highlighter. Peak chroma is only **0.130** — that
restraint is why it reads "mellow honey" not "warning yellow":
`amber-300` `oklch(0.855 0.095 72)` · **`amber-500` `oklch(0.735 0.130 62)` (primary)**
· `amber-700` `oklch(0.525 0.105 50)`. Used sparingly — active nav, primary buttons,
recording-idle halo, focus rings, kbd chips, heatmap peaks.

**Recording (warm vermilion):** `recording-300` `oklch(0.760 0.155 25)` →
`recording-500` `oklch(0.595 0.215 22)` — saturated enough to read urgently while
staying in the warm family (hue 22–25). **Semantic:** `success`
`oklch(0.755 0.135 155)` (cooler green, slightly breaks the warm scheme), `error`
`oklch(0.640 0.205 25)`.

### Warm-paper light theme
The light theme most fully embodies "soft brown/yellow" — **cream paper with darkened
honey ink**, Anthropic-adjacent. Surfaces become cream at hue 78–80 (`surface-0`
`oklch(0.985 0.005 80)`); text inverts to **dark brown** (`text-primary`
`oklch(0.215 0.012 65)`); light ambers are darkened for legibility on cream
(`amber-400` remapped `0.795→0.565` lightness).

### Shadow ramp, type, geometry
- **Shadows:** soft, multi-layered. Dark = black-based; **light = tinted warm brown
  `rgb(80 60 30)`** at low opacity — an unusual, deliberate choice reinforcing the
  warm/paper identity.
- **Type:** `Archivo` display (tight tracking `-0.022em`), `Outfit` body
  (`0.9375rem`, lh ~1.55), `IBM Plex Mono` for timers/metadata/kbd; tight **1.2**
  type scale; global `font-feature-settings: cv11, ss01, ss03`; `tabular-nums` helper.
- **Radii:** `rounded-xl` (0.75rem) workhorse for cards; inputs `lg`, buttons `md`,
  chips/toggles `full`. Overlay adds pill=999px, panels=14px, popups=10px.

### Why it reads "soft and brown"
(1) every neutral carries a warm hue-70 undertone at near-zero chroma; (2) the only
saturated brand color is a deliberately desaturated amber capped at 0.130 chroma; (3)
the light theme literally is warm paper; (4) shadows are soft and (in light)
brown-tinted; (5) the overlay's charcoal base (`rgba(28,26,24)→(22,21,20)`) is a warm
brown-black. Nothing is cold, high-chroma, or hard-edged.

---

## 4. Interaction & Motion Patterns (Shared Vocabulary)

- **Nav active states.** Sidebar `NavButton`: idle muted/transparent; hover
  `bg-surface-2/60`; active `text-amber-300` + `bg-amber-500/[0.08]` + a **left rail
  indicator** (2.5px `amber-400` bar). Page tab bars use a bottom underline that
  animates **opacity only**, not width.
- **Recording state machine.** Shared lifecycle **idle → recording → processing →
  success/error → idle**, surfaced identically on the 96px main `RecordButton` and the
  overlay pill: idle amber halo (hover `scale-1.025`, `active:scale-0.97`); recording
  `recording-pulse` (2.4s) + expanding `recording-ring` box-shadow; processing amber
  spin; success flashes first ~30 chars 2.5s; error red border + "!".
- **Waveforms.** `AudioVisualizer` (main, 5 bars, amber gradient, `bar-bounce` 0.85s
  staggered); `PillWaveform` (12 bars, bell-curve weights + per-bar phase, mode-colored);
  `IdleWaveform` (5 bars, `idle-wave` 2.4s, low opacity). Structuring = violet family
  (`structuring-halo/spark/shimmer/dot`).
- **Toasts.** Bottom-right `z-50`, max 5, `slide-up`, auto-dismiss 6s/10s, dedup by
  `code`. error/warn/info level styling.
- **Easing hierarchy.** `cubic-bezier(0.22,1,0.36,1)` = entrances (fade/slide/scale);
  `cubic-bezier(0.16,1,0.3,1)` = spring pop-ins (menus, popups); `cubic-bezier(0.4,0,
  0.6,1)` = recording loops; linear = spinners.
- **Hover/focus.** Cards brighten border + solidify bg over 200ms; toggles `h-[22px]
  w-10`, off `surface-3` / on `amber-500` (violet for LLM). **Global focus-visible:**
  `2px solid amber-500`, 2px offset. Overlay quick-toggles use `onMouseDown` (not
  click) to dodge WebView2 transparency click-through, optimistic + revert on failure.
- **Keyboard.** Global push-to-talk (LCtrl+LAlt) + Right-Ctrl Command Mode are
  OS-level. In-UI: Enter submits, Esc closes, Cmd/Ctrl+Enter pastes in Structured
  Panel, copy/save flash a green Check.

---

## 5. Per-Surface Notes

**Dictation Page** — Centered flex column on a radial-gradient bg; fixed 2rem Archivo
headline that changes color by state. 96px RecordButton focal; below it `max-w-lg`
stats / feature-tip / last-transcription cards. Error state is a bare "Something went
wrong" with no recovery affordance.

**History Page** — Sticky header + 250ms debounced search; cards with hover-reveal
copy/delete; "Load more" at PAGE_SIZE=50; empty state = amber-tinted Clock circle.
**Delete is immediate, no confirmation.** Doesn't animate cards (Notes does).

**Notes Page** — Grid view (`grid-cols-2 lg:grid-cols-3`, staggered fade-in,
`line-clamp-4`, hover lift) vs. fullscreen editor (borderless 2rem title + amber rule +
`min-h-[60vh]` textarea). Debounced 1.5s auto-save; dictation appends with smart spacing.

**Dictionary Page** — Three tabs with animated amber underline; identical inline-CRUD
pattern (add row, hover-reveal Pencil/Trash, Check/X). Snippet triggers in kbd styling;
Words use ArrowRight separator. Silent validation, immediate deletes.

**Context Modes Page** — `max-w-3xl` mode cards (colored icon container, "Active" badge);
edit form unlocks nested Dictionary/Snippets/App-Bindings tables. 13-icon picker +
6-color circle picker. A commented-out "AI Cleanup Prompt" textarea hints at a deferred
per-mode system-prompt feature.

**Models Page** — Speech (amber) and LLM Structuring (violet) tabs. **The violet uses
standard Tailwind violet, not the tuned OKLCH system**, so it reads brighter than the
rest. Cards = name+badges / metadata / action + 3px left accent stripe. Key-based
remount on tab switch discards test results.

**Analytics Page** — Six staggered cards: ledger overview, 52-week heatmap, peak-hours +
models (2×2), 30-day trend. Charts use a 5-step amber `color-mix()` ramp (`heat-0`=
surface-2 → `heat-4`=amber-400 with glow); bars use `bar-empty/fill/fill-strong`. Light
theme darkens the ramp to amber-700 ("business gold"). No chart ARIA, no loading state.

**Settings Page** — CSS-columns masonry of `GroupCard`s with cascade delays. Hotkey
recorder is a 3-state machine **signaled by color alone** (amber listening / green
confirming), no icon/label change. Mixed disclosure styles. Voice Commands opens a
centered modal. Theme toggle is the single point that flips `data-theme`.

**Floating Pill & Overlay** — Locked to `data-theme="dark"`; warm-charcoal base with
atmospheric depth (radial blooms, 1px rim-light insets, SVG `feTurbulence` grain ~0.4
opacity, accent glow). Dynamically resized per state (56×26 idle → 210×34 active →
600×~240 menu → 420×480 structured) via `useOverlaySizing.ts` with an asymmetric
collapse-instant/expand-after-80ms trick to avoid WebView2 flicker. **Quick-toggle
circles and popups use hand-crafted RGBA gradients, not OKLCH tokens** — e.g. the
brown→honey toggle gradient `rgba(148,98,18,0.28)→(130,84,14,0.22)` off,
`rgba(200,142,36,0.86)` on (`FloatingPill.css`).

**Structured Panel** — 420px panel clipping open from the pill via reverse-Dynamic-Island
clip-path (`sp-in` 420ms). Header kicker + metadata chips + scrollable Markdown body +
collapsible raw-transcript drawer + action row (Paste/Raw/Copy/Edit/Dismiss + in-panel
mic). **Primary Paste button is violet** — same hue as the Ley Line on-state (hue
overloading). In-panel dictation collapses buttons and expands the mic to a waveform bar.

**Command Pill** — **Deliberately breaks the warm palette with cool indigo**
(`ACCENT_RGB = "129,140,248"`, inline RGB — not in tokens) to signal "execution mode ≠
dictation." Three-zone layout; states listening/recognizing/confirm(300×44 with Yes-No
circles)/done(green, 2600ms)/error. No light-theme variant, no `aria-live`, no confirm
timeout.

**Shared** — ToastContainer is the lone reusable feedback primitive; ErrorBoundary is
full-screen with "Reload UI". Both lack ARIA. Plus the Tauri bridge (`useTauriEvent`,
`useTauriCommand`), these are the only cross-cutting UI — **there is no shared
Button/Input/Modal/Tooltip library.**

---

## 6. Cross-Cutting Observations for the Overhaul

*Prioritized high → low. Descriptive, not prescriptive.*

### A. Missing shared primitives (highest-leverage — blocks everything else)
1. **No component library.** Buttons, inputs, toggles, segmented controls, cards, empty
   states, tooltips are re-implemented inline on every page with drifting padding
   (`px-3` vs `px-4`), radii (`rounded-lg` vs `rounded-xl`), and ad-hoc hover states. A
   redesign should build shared `Button/Input/Toggle/Segmented/Card/EmptyState/Panel`
   primitives **before** touching pages, or every surface needs hand-retooling.
2. **No modal/dialog/tooltip system.** Only "modals" are ErrorBoundary + Settings
   Voice-Commands popup; tooltips are native `title`. No focus trap, no shared
   positioning, no `z-index` strategy.

### B. Tokenization gaps (brittleness against the overhaul)
3. **Hardcoded colors bypass tokens.** `FloatingPill.css` / `StructuredPanel.css` raw
   `rgba()` gradients; `CommandPill` inline indigo; `Logo.tsx` hex
   (`#f59e0b/#d97706/#b45309`); `MODE_COLORS` RGB strings. Editing `tokens.css` will
   **not** propagate to these — migrate them first so the palette lives in one place.
4. **`color-mix()` heatmap/bar blends auto-adapt** to a new accent — a strength to keep.

### C. Dark / light parity
5. **Light theme under-tested on interactive surfaces.** Inputs hardcode `bg-surface-2`
   with no `light:` variants; `text-amber-200` toasts, `recording-300` timers, and
   `border-amber-400/35` risk poor contrast on cream; light card borders (`oklch 0.890`)
   nearly invisible.
6. **Overlay is dark-only by design** (`data-theme="dark"` hardcoded) — no OS parity for
   the most-seen surface; Command Pill / popups have no light path.

### D. Accessibility
7. **No `aria-live` anywhere** — toasts, command-pill state, download progress,
   recording state invisible to screen readers; charts have no ARIA.
8. **Mouse-only interactions** — overlay quick-toggles (`onMouseDown`, `title`-only),
   History copy/delete, confirm buttons have no keyboard path.
9. **One-size focus ring** (`2px amber-500`) — oversized on 26px toggles; amber-on-amber
   states may fail WCAG AA.
10. **Color-only signaling** — hotkey recorder (amber/green), waveform mode colors (6
    hues by saturation not lightness), delete-intent (reddens on hover only).

### E. Inconsistencies & micro-drift
11. **Accent proliferation / hue overloading** — amber (brand), recording vermilion,
    **violet (Structured on-state AND primary Paste — two roles)**, indigo (Command),
    green (success); Models LLM violet is un-tuned Tailwind. *The central palette
    decision: unified warm ramp vs. an intentional, documented multi-accent system.*
12. **Motion inconsistency** — card entrance on Notes/Models/Analytics/Settings but not
    History; copy-icon swap is abrupt; add vs edit form chrome differs; icon sizing has
    no scale (10–36px, strokeWidth 1.5/2/2.5 ad-hoc).
13. **Sidebar indicators** use two visual languages (1.5px recording dot vs 2.5px nav bar).
14. **Destructive actions lack confirmation** across History/Notes/Dictionary/Modes —
    immediate, irreversible deletes, no undo.
15. **Silent error handling** — most CRUD failures `console.error` only, no toast.

### F. Dated / "soft" patterns to evaluate (taste calls)
16. **Warm-charcoal overlay + SVG grain + breathing loops** read "premium hardware /
    vintage desktop" — distinct, but compare against contemporary cold-neutral / frosted
    dark UIs if "2026-modern" is the goal. Grain + bespoke clip-path also carry GPU cost.
17. **Tight 1.2 type scale** is coherent but occasionally cramped; small-caps labels
    (`10.5px`, `0.12em`) easy to skim past.
18. **Subtle borders / no card elevation** (`border` = surface-3) make cards hard to
    distinguish from bg; hierarchy rests on size + opacity, not weight/color/shadow.

### G. Biggest opportunities
- **Build primitives first**, then re-skin pages from them.
- **Decide the accent strategy deliberately** and bring Command indigo + Models violet
  into the token system either way.
- **Reach full token coverage** (migrate overlay/panel/logo/mode-color holdouts).
- **Establish an accessibility baseline** (aria-live, keyboard paths, scoped focus
  rings, WCAG AA on both themes).
- **Preserve genuine strengths:** the OKLCH warm-low-chroma system, the Archivo/Outfit/
  IBM Plex stack, the 6-state recording clarity, the dual waveforms, the
  reverse-Dynamic-Island panel, the 56×26 idle footprint.
