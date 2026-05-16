# OmniVox Floating Pill — Redesign + Scratchpad Trigger (v1)

**Status:** proposal, not yet implemented. To be reviewed by user + Codex before any code change.

**Branch:** TBD — likely `feat/pill-slim-and-scratchpad` off current `codex/rust-health-screen-context-audit` or a fresh branch from `main`.

**Goal:** (1) slim the floating pill's default idle state so it reads as a sleeker baseline, (2) make the right-click context menu's pill expansion respect the new sleeker profile rather than auto-jumping to the active-dictation thickness, and (3) add a new hover-only trigger button to the right of the pill that opens a small companion "scratchpad" window — a future home for quick utilities (clipboard, notes, mode shortcuts, etc.). This v1 ships the slimming and the trigger plumbing only; the scratchpad window itself is a shell with placeholder content.

---

## Context: why this plan exists

The pill today (`src/features/overlay/FloatingPill.tsx`) has three visible states with dimensions hard-coded near the top of the component:

| State              | Width  | Height | Notes                                              |
|--------------------|--------|--------|----------------------------------------------------|
| Idle               | 56 px  | 26 px  | 5-bar ambient waveform, taskbar-floating           |
| Active (dictating) | 210 px | 34 px  | Timer + 12-bar waveform + status dot               |
| Right-click menu   | 210 px | 34 px  | Pill widens to active size; menu floats above pill |

The right-click menu currently inherits the **active dictation** profile (210×34) even though no recording is happening. After the slim redesign that mismatch becomes glaring — a slim pill should not balloon to dictation-thickness just to open a contextual menu. Active-dictation thickness should remain reserved for "we are actually capturing audio right now."

Separately, the user wants a small extensible side surface — a "scratchpad" — that is launchable from the pill but doesn't clutter the always-on overlay. The simplest UX: a circle to the right of the pill that only reveals itself when the user is already attending to the pill (hover), then opens a second window on click. The pill stays the central, minimal-presence element; the scratchpad is a deliberate sidecar.

---

## 1. State model — four pill states (was three)

This redesign splits idle into **slim-idle** and **hover-idle** and adds a dedicated **menu** state with its own thickness:

| State          | Pill W×H   | Window W×H            | Trigger circle visible | Triggered by                                |
|----------------|-----------|-----------------------|------------------------|---------------------------------------------|
| **slim-idle**  | 56 × 22   | 56 × 22               | no                     | default; mouse not over overlay window      |
| **hover-idle** | 56 × 26   | 86 × 26 (asymmetric)  | yes (22 × 22, right)   | `mouseenter` on overlay window in idle      |
| **menu**       | 210 × 26  | 600 × ~240 (unchanged height of menu floats above) | no | right-click on pill (when idle/success/error) |
| **active**     | 210 × 34  | 210 × 34              | no                     | left-click on pill OR hotkey                |

Key thickness ladder: **22 → 26 → 26 → 34**. The slim/hover/menu states share a "not recording" feel (22–26 px); active jumps to 34 to clearly mark "audio is being captured." This is intentional and is the whole point of the redesign.

### 1.1 Why these specific numbers

- **22 px slim-idle:** confirmed via user question. 4 px slimmer than today's 26. The 5-bar idle waveform's max bar height drops from 10 px to ~7 px to keep proportion. Bars stay 2 px wide with 3 px gap (unchanged) — slimming the waveform height alone is enough to read as "slimmer."
- **26 px hover-idle:** matches today's idle height exactly. Confirmed via user question. Familiar visual; the only change is that the circle appears to its right.
- **210 × 26 menu pill:** widens to the active length so the menu has a stable parent rail to align to (the ModeSelector and toggle popups are already positioned relative to a 210 px-wide pill in the existing code), but **does not** thicken to 34. This is the "fits the new thickness" requirement.
- **210 × 34 active:** unchanged. User explicitly OK'd leaving this alone. The thickness contrast (now 22 → 34 instead of 26 → 34) actually *strengthens* the active affordance.

### 1.2 Trigger circle dimensions

- **Size:** 22 × 22 px (matches slim-idle height exactly, so the trigger sits flush at the pill's mid-line in hover-idle).
- **Gap from pill:** 8 px.
- **Shape:** `border-radius: 999px` (full circle).
- **Icon:** Lucide `StickyNote` at 12 px, centered. (Alternatives: `Notebook`, `Pencil`, `PenLine`. `StickyNote` reads as "scratch / notes" most clearly at 12 px on a 22 px target.)
- **Color:** matches the existing `.quick-toggle` amber-gradient family used for the right-click toggle row, so it feels like the same control vocabulary:
  - Resting (within hover-idle): `linear-gradient(180deg, rgba(148,98,18,0.28) 0%, rgba(130,84,14,0.22) 100%)` at 0.7 opacity.
  - Mouse over the circle itself: 1.0 opacity + slightly brighter gradient.
- **Visibility:** opacity-driven, not display-toggled. Resting opacity 0 (in slim-idle), animates to 0.7 over 180 ms when hover-idle activates. Always laid out in the DOM but pointer-events disabled when transparent.

---

## 2. Visual + motion specifications

### 2.1 Pill background, border, radius

No change. The pill keeps `bg-[var(--color-pill-bg)]`, `rounded-full`, and the existing 1 px transparent border. Only `width` and `height` change per state.

### 2.2 Idle waveform recalibration

`IdleWaveform` (`src/features/overlay/FloatingPill.tsx:1543`) currently animates 5 bars between 2 and 10 px height. To fit 22 px:

- **Container max-height:** 7 px (down from 10).
- **Animation range:** 2 → 7 px (was 2 → 10).
- **Bar count, width, gap:** unchanged (5 bars, 2 px wide, 3 px gap → 22 px total width, comfortably inside 56 px pill).
- **Animation duration/easing:** unchanged (`idle-wave 2.4s ease-in-out` with per-bar phase offsets).

In **hover-idle** (26 px tall), the waveform should *return to today's 10 px max-height*. Implementation: pass `maxBar` (or similar) as a prop to `IdleWaveform`, e.g. `<IdleWaveform maxBar={isHovering ? 10 : 7} />`. Animation continues running across the state change so we get a smooth-feeling breath rather than a hard snap.

### 2.3 Motion budget

All state transitions on the pill share the existing pattern in `FloatingPill.tsx:189–259` (hide content → resize window → show content with fade-in). The new transitions:

| Transition                  | Duration | Easing                       | Notes                                            |
|-----------------------------|----------|------------------------------|--------------------------------------------------|
| slim-idle → hover-idle      | 180 ms   | `cubic-bezier(0.2, 0.8, 0.2, 1)` | Window expands rightward; pill grows top+bottom; trigger fades in 0 → 0.7 |
| hover-idle → slim-idle      | 140 ms   | `cubic-bezier(0.4, 0, 0.6, 1)`   | Slightly faster than the expand; trigger fades out first (60 ms head start) so it's gone before window shrinks |
| hover-idle → active         | as today | unchanged                        | Click; window expands to 210 × 34, trigger fades out |
| slim-idle → menu (right-click) | as today | unchanged                     | Window expands to 600 × ~240; pill becomes 210 × 26 |
| menu → slim-idle (close)    | as today | unchanged                        | Reverse                                          |

**Hover hysteresis:** when the mouse leaves the *expanded* window's bounds in hover-idle, wait 120 ms before initiating slim-idle collapse. This prevents flicker when the user grazes the edge or moves between the pill and the trigger circle. If the mouse re-enters within the 120 ms window, cancel the collapse. Use a single `setTimeout` ref, cleared on `mouseenter`.

### 2.4 Pill stays at screen-center horizontally

Critical requirement from user: "the pill should stay centered as is." The trigger circle expands the window asymmetrically rightward so the **pill itself does not shift** when hover-idle activates.

Two equivalent implementations — pick one in code:

- **Option A (recommended):** in the Rust `resize_overlay` command (`src-tauri/src/commands/settings.rs:78`), accept an optional `anchor: "pill-center"` parameter. When set, position the window so that *pill-center-x* (not window-center-x) equals the cursor monitor's center-x. The frontend computes `pill_offset_x = 0` for slim-idle/active/menu (pill fills the window) and `pill_offset_x = 0` for hover-idle (pill is at the left of the window, trigger circle to the right).
- **Option B:** Frontend always asks for a centered window; backend stays unchanged. In hover-idle, add invisible left padding equal to (trigger width + gap) so the pill sits at window-center while the trigger is at the right. Window width = 22 + 8 + 56 + 8 + 22 = 116 px. Wastes 30 px of overlay window but no Rust change.

**Recommendation: Option A.** It's a 10-line Rust change and keeps the overlay window's hitbox tight to actual content. Option B leaks an invisible 30 px transparent strip to the left of the pill that could swallow clicks intended for other apps.

### 2.5 What does NOT change

- Right-click menu width (600 px), ModeSelector height formula (`modes.length * 34 + 40 + 34`, clamped to 240), quick-toggle row (5 circles at 26 × 26), and popup widths (168 / 160 px) all remain.
- StructuredPanel (440 × 480) remains.
- Active-dictation styling (border colors per state, padding, gap-2.5, 12-bar waveform). User explicitly OK'd leaving this alone.
- Window decorations, transparency, always-on-top, skip-taskbar — all unchanged.

---

## 3. Scratchpad window — shell only

Per user decision: this v1 defines the window plumbing only. The scratchpad's content is a placeholder div with the title "Scratchpad" and a small `// future home for quick utilities` comment in the rendered HTML. Subsequent PRs add features.

### 3.1 Window characteristics

| Property         | Value                                                  |
|------------------|--------------------------------------------------------|
| Tauri label      | `scratchpad`                                           |
| Entry HTML       | `scratchpad.html` (new, sibling of `overlay.html`)     |
| Initial size     | 320 × 420 px                                           |
| Min size         | 240 × 280 px                                           |
| Initial position | anchored above the pill, right-aligned to the trigger circle (so it visually "drops from" the trigger). If that lands off-screen, fall back to centered. |
| Decorations      | `false` (custom titlebar with drag region + close button, matching the overlay's borderless aesthetic) |
| Transparent      | `true` (so we can do a rounded-corner panel)           |
| Always-on-top    | `true` (so it doesn't disappear behind the dictation target app — matches pill behavior) |
| Skip-taskbar     | `true`                                                 |
| Resizable        | `true`                                                 |
| Visible at start | `false` (created on app boot, shown on first toggle)   |
| Focused          | follows show — `set_focus()` after `show()`            |

### 3.2 Open/close behavior

- **Click trigger circle:** if hidden → show + focus; if shown → hide.
- **Click outside / blur:** do **not** auto-close. Scratchpad is intended to be a persistent sidecar the user can leave open while working in another app. Closing is explicit (titlebar X or click trigger again).
- **App quit:** scratchpad window is destroyed with the app.
- **Recording starts (any path):** scratchpad stays open. It is independent of dictation state.

### 3.3 Wiring

New files:

- `scratchpad.html` — minimal Vite entry, mirrors `overlay.html`.
- `src/scratchpad-main.tsx` — React mount, mirrors `src/overlay-main.tsx`.
- `src/features/scratchpad/Scratchpad.tsx` — shell component. Renders a titlebar + close button + empty content area. ~30 lines.

Backend additions:

- `src-tauri/src/lib.rs` — new `setup_scratchpad_window()` called after `setup_overlay_window()` in `run()`.
- `src-tauri/src/commands/settings.rs` (or a new `commands/scratchpad.rs` if it grows) — `toggle_scratchpad`, `show_scratchpad`, `hide_scratchpad` commands. Only `toggle_scratchpad` is wired this PR.
- `tauri.conf.json` — no change (window is built programmatically, same as overlay).

Frontend bridge:

- `src/lib/tauri.ts` — add `toggleScratchpad(): Promise<void>` wrapping `invoke("toggle_scratchpad")`.

Vite multi-page config:

- `vite.config.ts` — add `scratchpad: "scratchpad.html"` to `rollupOptions.input` (mirroring how `overlay.html` is currently registered).

### 3.4 Why a second window, not an inline expansion

The existing `StructuredPanel` is an inline expansion (renders above the pill within the same overlay window). For the scratchpad we want a **separate** window because:

1. **Persistent across dictation cycles** — inline panels are gated on the pill's `showContent` flag and get hidden during the hide→resize→show cycle. A scratchpad that flickers every time the user starts a recording is unusable.
2. **Independently positionable** — user can drag it wherever; an inline panel is locked above the pill.
3. **Resizable** — the overlay window is sized to its content; you can't resize an inline panel without contorting the resize logic.
4. **Memory cost is bounded** — Tauri 2 webview windows share a process, and the scratchpad bundle is small (no whisper / no LLM JS, just a textarea-ish surface eventually). The same reasoning that justified `overlay.html` as a separate window justifies this one.

---

## 4. Implementation plan — file-by-file

### 4.1 `src/features/overlay/FloatingPill.tsx`

The single big file. Changes:

- **Lines 52–55 — dimension constants:** add new constants, keep old ones for active state.
  ```ts
  const SLIM_IDLE_W = 56;
  const SLIM_IDLE_H = 22;
  const HOVER_IDLE_W = 86;   // 56 pill + 8 gap + 22 trigger
  const HOVER_IDLE_H = 26;
  const MENU_PILL_W = 210;
  const MENU_PILL_H = 26;
  const ACTIVE_W = 210;
  const ACTIVE_H = 34;
  ```
- **New state:** `const [isHovering, setIsHovering] = useState(false);` plus a ref-stored hover-leave timeout for hysteresis.
- **Event handlers on the root pill element:**
  ```ts
  onMouseEnter={() => {
    if (hoverLeaveTimer.current) clearTimeout(hoverLeaveTimer.current);
    if (pillState === "idle" && !showModeSelector) setIsHovering(true);
  }}
  onMouseLeave={() => {
    hoverLeaveTimer.current = setTimeout(() => setIsHovering(false), 120);
  }}
  ```
- **Lines 189–259 — the consolidated resize effect:** add `isHovering` to the dependency list and the size-selection logic. Order matters — `pillState === "active"`, `showModeSelector`, `structuredPayload` all dominate `isHovering`. The hover state only applies when the resolved base state is idle.
- **Render branch for the trigger circle:** new JSX inserted alongside the pill (sibling, not child, so it sits outside the pill's rounded background). Wrapped in a `<div>` with `display: flex; align-items: center; gap: 8px;` that contains both the pill and the trigger.
- **Lines 1166–1506 — embedded `<style>` block:** add `.scratchpad-trigger` and `.scratchpad-trigger:hover` rules (see §1.2 for the gradient). Add `.scratchpad-trigger--visible` class that animates opacity 0 → 0.7 over 180 ms.
- **Lines 1543–1571 — `IdleWaveform`:** accept a `maxBar` prop (default 7). Replace the hardcoded `10` with the prop in the animation keyframes' `to` height. Update both the inline style and the embedded `@keyframes idle-wave` rule — that keyframe is shared, so make `--idle-wave-max` a CSS custom property set per-instance and reference it in the keyframe (`height: var(--idle-wave-max)`).

### 4.2 `src-tauri/src/lib.rs`

- Add `fn setup_scratchpad_window(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>>` modeled on `setup_overlay_window` (`lib.rs:43–82`). See §3.1 for parameters.
- Call it from `run()` after the existing `setup_overlay_window()` call.

### 4.3 `src-tauri/src/commands/settings.rs`

- Add `#[tauri::command] async fn toggle_scratchpad(app: tauri::AppHandle) -> Result<(), String>`.
  - `app.get_webview_window("scratchpad")` → if `is_visible()` then `hide()` else `show()` + `set_focus()`.
- (Optional, defer) Extend `resize_overlay` with an `anchor: Option<&str>` parameter for Option A in §2.4. If we pick Option A, this change happens in the same PR.
- Register the new command in `invoke_handler!` (`lib.rs`).

### 4.4 `src/lib/tauri.ts`

- Add `export async function toggleScratchpad(): Promise<void> { return invoke("toggle_scratchpad"); }`.

### 4.5 `vite.config.ts`

- Add `scratchpad: resolve(__dirname, "scratchpad.html")` to the `build.rollupOptions.input` map (same shape as the overlay entry).

### 4.6 New files

- **`scratchpad.html`** — copy of `overlay.html` with `<title>Scratchpad</title>` and `<script type="module" src="/src/scratchpad-main.tsx"></script>`.
- **`src/scratchpad-main.tsx`** — copy of `src/overlay-main.tsx` swapping the rendered component for `<Scratchpad />`.
- **`src/features/scratchpad/Scratchpad.tsx`** — minimal shell:
  ```tsx
  export function Scratchpad() {
    return (
      <div className="scratchpad-window">
        <header className="scratchpad-titlebar" data-tauri-drag-region>
          <span>Scratchpad</span>
          <button onClick={hideScratchpad}>×</button>
        </header>
        <main className="scratchpad-body">
          {/* future home for quick utilities */}
        </main>
      </div>
    );
  }
  ```

---

## 5. State machine — what triggers what

```
                              right-click
              ┌───────────────────────────────────────┐
              │                                        │
              ▼                                        │
  ┌──────────────────┐  mouseenter   ┌──────────────────┐
  │   slim-idle      │ ────────────▶ │   hover-idle     │
  │   56×22          │               │   pill 56×26     │
  │                  │ ◀──────────── │   trigger 22×22  │
  └──────────────────┘  mouseleave   └──────────────────┘
        │     ▲              (120ms hysteresis)   │
        │     │                                   │
        │     │ recording finished                │
        │     │ (+ flash text, then back)         │ click pill
        │     │                                   │
        │     │                                   ▼
        │     │                          ┌──────────────────┐
        │     └──────────────────────────│     active       │
        │                                │     210×34       │
        │ right-click                    └──────────────────┘
        │                                         ▲
        ▼                                         │
  ┌──────────────────┐                            │ (hotkey while slim)
  │      menu        │                            │
  │   pill 210×26    │ ───────────────────────────┘
  │   window 600×~240│   left-click toggle while menu open
  └──────────────────┘
```

Click on the trigger circle is a separate side-effect (toggles scratchpad window visibility) — it doesn't change the pill's state, so it's not represented above. The pill stays in hover-idle while the cursor is over the trigger.

Edge cases:
- **Hotkey starts recording while in hover-idle:** transitions directly to active. Trigger fades out as part of the resize.
- **Hotkey starts recording while in slim-idle:** transitions to active (same as today). Hover state is moot since cursor isn't over the pill.
- **Right-click while in hover-idle:** transitions to menu. Trigger disappears (menu state has no trigger).
- **Right-click while in active/recording:** blocked, same as today (`FloatingPill.tsx:545–579`).
- **Scratchpad window open + user starts recording:** scratchpad stays as-is, independent.

---

## 6. Risks and open questions

### 6.1 Mouse-enter false positives

The overlay window is always-on-top and sits centered above the taskbar. Users mouse over it incidentally when reaching for the system tray. If hover-idle expansion is too eager, the pill will flicker for every accidental cursor pass.

**Mitigation:** require a 100 ms dwell before activating hover-idle. Track `mouseenter` + `setTimeout(activate, 100)`, cancel on `mouseleave`. If the user actually intends to interact with the pill they'll dwell for >100 ms; a fly-by cursor pass won't trigger. This is in addition to the 120 ms `mouseleave` hysteresis.

Combined budget: 100 ms in + 180 ms expand + 120 ms hysteresis out + 140 ms shrink = the pill is visibly "alive" for ~500 ms minimum per intentional hover. Worth validating with a quick prototype before committing.

### 6.2 Pill-center anchoring when the cursor changes monitors

`resize_overlay` currently follows the cursor's monitor (`commands/settings.rs:78–150`). If pill-center anchoring is implemented (§2.4 Option A), it needs to keep using the cursor's monitor as the reference frame. Verify on multi-monitor setups that hovering doesn't trigger a window-jump to a different monitor — this is already a concern with the existing resize logic but the hover state makes it more frequent.

### 6.3 Trigger circle hit zone vs. pill hit zone

The trigger circle (22 × 22) and the pill share a single window. The pill's `onClick` toggles recording. The trigger's `onClick` toggles scratchpad. These are sibling DOM elements — `event.stopPropagation()` on the trigger's click handler is sufficient, but worth testing that right-click on the trigger doesn't accidentally open the mode selector.

**Decision:** right-click on the trigger should do **nothing** (`onContextMenu={(e) => e.preventDefault()}`). The mode selector is for the pill only.

### 6.4 Slim waveform legibility

Going from 10 px to 7 px max-bar-height is a 30% reduction. At typical viewing distance (laptop, ~50 cm) the bars at minimum height (2 px) may become hard to perceive. If this is a problem in practice, raise to 23 × 56 (slim height 23 px, max bar 8 px) rather than going back to 26. Defer to a quick visual check after first implementation.

### 6.5 Codex-flag: is 210 × 26 the right menu thickness?

Two alternative numbers I considered and rejected:

- **210 × 28:** matches `26 + 2` for a hairline thickness boost in menu state to differentiate from hover-idle. Rejected because it's a subtle distinction without a real purpose — there's no scenario where the user is in both hover-idle and menu state simultaneously.
- **210 × 22:** matches slim-idle thickness exactly. Most "fits the new thickness" interpretation. Rejected because the menu's mode list extends downward from the pill; if the pill is too thin the visual anchor for the menu's column feels detached. 26 keeps the pill substantial enough to feel like a header rail.

If Codex disagrees, easy single-constant change.

### 6.6 Out of scope for this PR

- Scratchpad content/features (textarea, clipboard history, quick mode toggles, etc.) — separate PR(s).
- Scratchpad persistence (where notes are stored, what format) — defined when scratchpad content lands.
- Keyboard shortcut to open scratchpad — defer; click-from-pill is the v1 entry point.
- Telemetry/analytics for hover-trigger usage — not currently instrumented elsewhere in the app, no reason to start here.
- Light-mode tuning of the trigger circle's amber gradient — verify on light theme before merging but don't redesign.

---

## 7. Acceptance criteria

A reviewer (or Codex) should be able to verify:

1. **Slim-idle:** with the app idle and no cursor over the overlay, the pill measures 56 × 22 px and the waveform tops out at 7 px.
2. **Hover-idle:** moving the cursor onto the pill (and holding for ~100 ms) expands the pill to 56 × 26 and reveals a 22 × 22 amber circle 8 px to its right. The pill's horizontal screen position does not shift during the transition.
3. **Trigger click:** clicking the circle opens a 320 × 420 window labeled "Scratchpad" with a draggable titlebar and close button. Clicking the circle again hides the window. Closing the titlebar X hides the window.
4. **Right-click menu:** right-clicking the slim or hover-idle pill opens the mode selector. The pill widens to 210 × 26 (not 210 × 34). The toggle row and popups still align correctly.
5. **Active dictation:** left-clicking the pill or pressing the hotkey starts recording. The pill expands to 210 × 34. The trigger circle is not visible during recording.
6. **Hysteresis:** moving the cursor briefly off the pill and back (within 120 ms) does not trigger a slim-idle collapse. Moving off for longer collapses cleanly with no flicker.
7. **No regressions:** structured panel, voice commands, mode switching, ghost mode, success/error states, degraded-banner dismissal — all behave identically to today.

---

## 8. Suggested PR order

1. **PR 1 — slim states only:** constants, hover state, trigger circle JSX wired to a no-op `console.log`, menu thickness fix. No Tauri changes. Reviewable in isolation.
2. **PR 2 — scratchpad window:** new Tauri window + entry HTML + React shell + `toggle_scratchpad` command + wire trigger click. Builds on PR 1.

PR 2 depends on PR 1 but each is independently testable. Both target the same release.
