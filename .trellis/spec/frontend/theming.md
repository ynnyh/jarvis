# Theming System (CSS Variable Tokens)

> How visual themes work and how to add/extend them safely.
> Source of truth: `desktop/src/style.css`. Composable: `desktop/src/composables/useTheme.ts`. Registry: `desktop/src/style-themes.ts`.

---

## Overview

The app ships 6 visual themes, switchable at runtime with **zero re-render**:

| id | name | character |
|----|------|-----------|
| `sci-fi` | 霓虹风 | magenta neon × deep-purple night (default) |
| `playful` | 俏皮风 | candy pastel, big radius, bouncy shadow |
| `zen` | 治愈风 | paper/wood, low-saturation beige |
| `minimal` | 极简风 | grayscale mono, hairline borders, no shadow/motion |
| `matrix` | 矩阵风 | hacker green × near-black, scanlines, canvas digital-rain |
| `cyber` | 未来风 | deep-space black × electric-blue/holo-purple, canvas particles |

Theming is driven entirely by **CSS custom properties (design tokens)**. There is no JS color logic in components. The two canvas-backed themes (`matrix`, `cyber`) additionally mount a self-contained `<canvas>` background component (see "Canvas-backed theme effects" below).

---

## Core Contract

1. **Tokens are the only switch surface.** `:root` holds the full default token set (default = `sci-fi`). Each theme overrides via `:root[data-theme="<id>"] { ... }`.
2. **Switching = set `<html data-theme>`.** `useTheme()` writes `document.documentElement.dataset.theme` and watches `config.styleTheme`:
   ```ts
   // useTheme.ts
   const apply = (id: string) => { document.documentElement.dataset.theme = id }
   apply(store.config.styleTheme)
   watch(() => store.config.styleTheme, apply)
   ```
   Because every component reads `var(--xxx)`, the CSS cascade repaints instantly — **no component re-render, no key bump**.
3. **Cross-window sync.** Each window is a separate WebView. `config_save` (Rust) emits `config-changed` to ALL windows → each store calls `load()` → `useTheme` watch fires → `data-theme` updates everywhere.

---

## Convention: every window entry MUST call `useTheme()`

There are 8 window entries; all must call it at setup top-level, or that window will not react to theme changes:

`App.vue`, `ChatApp.vue`, `CostApp.vue`, `SettingsDetailApp.vue`, `WriteHoursApp.vue`, `ManualHoursApp.vue`, `TodayPlanApp.vue`, `BatchWriteApp.vue`.

```ts
import { useTheme } from './composables/useTheme'
useTheme()
```

---

## Convention: components use `var(--xxx)`, NEVER hardcode colors

A hardcoded color does not follow the theme — it stays fixed across all 4 themes and breaks the look (and can hurt contrast in light themes).

```css
/* Wrong — frozen blue in every theme */
.badge-soon { background: rgba(59, 130, 246, 0.8); color: white; }

/* Correct — follows the active theme */
.badge-soon { background: var(--blue-bg-strong); color: var(--blue-text); }
```

**Allowed exceptions** (intentional, do not "fix"): functional state indicators that are orthogonal to UI theme (`Avatar.vue` AI-state colors: thinking/working/notifying/error), chart series palettes (`CostApp.vue` `COST_COLORS`), and the native `<select>` dropdown fallback (`#222 on #fff`, because OS dropdowns ignore most CSS).

---

## Token Vocabulary (key tokens)

- **Surfaces**: `--bg`, `--bg-2`, `--surface`, `--surface-2`, `--surface-hover`, `--surface-item-hover`, `--surface-item-active`
- **Text layers**: `--text` → `--text-ghost` → `--text-dim` → `--text-muted` → `--text-faint`
- **Accents**: `--accent`, `--accent-2`, `--accent-text`, `--accent-border`, `--accent-glow`, `--glow`
- **Status (each has `-text/-text-light/-bg/-bg-strong/-border` + base)**: `--red-*`, `--yellow-*`, `--blue-*`, `--green-*`, `--purple-*`
- **Panels/menus**: `--panel-bg`, `--panel-border`, `--panel-shadow`, `--panel-header-bg`, `--menu-bg`, `--menu-border`, `--menu-shadow`, **`--popup-bg`**
- **Inputs/buttons**: `--input-bg`, `--input-border`, `--input-focus-border`, `--btn-primary-bg/-color/-shadow`
- **Shape/motion**: `--radius-sm/md/lg/control`, `--shadow-1/2`, `--font-display`, `--num-font-variant`

---

## Gotcha: floating popups MUST use `--popup-bg`, not `--menu-bg`

> The avatar window is `"transparent": true` (tauri.conf.json). Any surface that floats inside it must be **fully opaque**, or background content / desktop bleeds through.

- `--menu-bg` is semi-transparent in some themes (e.g. sci-fi `rgba(13,11,26,0.88)`) and the panels have no `backdrop-filter` fallback → **content shows through**.
- `--popup-bg` is defined opaque in every theme (sci-fi bakes in a magenta/cyan radial glow over an opaque `linear-gradient(#0e0a17,#140a1d)` base).

```css
/* Wrong — 12% see-through, bleeds in the transparent avatar window */
.settings-panel { background: var(--menu-bg); }

/* Correct — opaque, theme-aware glow */
.settings-panel { background: var(--popup-bg); box-shadow: var(--panel-shadow); }
```

Use `--popup-bg` for: review/risk/bind/task popups, the right-click menu, the settings panel. Keep `--menu-bg` only for genuine small dropdowns that have their own shadow (e.g. `.iw-dropdown`).

### 同样适用于 `--panel-bg`：透明主窗里的悬浮面板背景一律用 `--popup-bg`

挂载在透明主窗（`App.vue`，root `background: transparent`）里的悬浮面板，主背景**必须**用 `--popup-bg`（在全部 6 套主题里都是不透明的）。

切勿把 `--panel-bg` / `--menu-bg` 当成悬浮面板的主背景：它们在极简（minimal）主题下被刻意做成近乎全透明（如 `--panel-bg: rgba(255,255,255,0.03)`，仅 3% 不透明度），只有当面板叠在不透明的窗口 body 之上（即独立窗口入口，如 ChatApp/BatchWriteApp/WriteHoursApp/TodayPlanApp/ManualHoursApp/CostRatesSection）时才安全。直接浮在 `App.vue` 透明 root 上会导致桌面透视——`UpdateWindow.vue` 曾因此在极简主题下几乎看不见（`.update-panel` 误用 `--panel-bg`，应为 `--popup-bg`）。`--panel-header-bg` 等薄叠加层不受影响，可继续用在不透明主背景之上。


---

## Convention: adding a theme = copy one `[data-theme]` block and override ALL differing tokens

```css
:root[data-theme="my-theme"] {
  /* must set every token that differs from sci-fi defaults */
}
```

Then register it in `style-themes.ts` (`STYLE_THEMES`). No component changes needed.

### Common Mistake: partial token override

**Symptom**: A light/grayscale theme unexpectedly shows saturated neon colors (e.g. `minimal` task cards show red/cyan/purple borders and colored badges despite being "grayscale mono").

**Cause**: Tokens NOT overridden in a theme block fall back to `:root` defaults (= sci-fi values). `minimal` only overrides the 5 `*-text` status tokens, so `--red/--yellow/--blue/--green/--purple` plus their `-bg/-bg-strong/-border` inherit the saturated defaults.

**Fix / Prevention**: When a theme's intent differs from sci-fi for a token family, override the **entire** family (`base + -text + -text-light + -bg + -bg-strong + -border`), as `zen` does for blue/green/purple.

---

## Pattern: theme-scoped semantic override (sci-fi blue → cyan)

**Problem**: In the neon theme, the "soon/临近" status uses the generic info-blue (`#3b82f6`), which clashes with the magenta×cyan palette.

**Solution**: Override the `--blue-*` family **only inside the `[data-theme="sci-fi"]` block** to cyan (`#00d4ff`). Do NOT touch the default `:root` `--blue-*`.

```css
:root[data-theme="sci-fi"] {
  /* ...other neon tokens... */
  --blue: #00d4ff;
  --blue-text: rgba(0, 212, 255, 0.95);
  --blue-bg-strong: rgba(0, 212, 255, 0.22);
  --blue-border: rgba(0, 212, 255, 0.40);
  /* etc. */
}
```

**Why scoped, not global**: `playful` and `minimal` only partially override `--blue-*` and inherit the rest from the default `:root`. Changing the default `--blue` would leak cyan into those themes. Scoping to the sci-fi block keeps the change contained; `zen` (full self-override) is unaffected regardless.

---

## Decorative background layer (`.theme-bg`)

Real (non-avatar) windows add class `.theme-bg` on their root to get `--theme-bg` plus a `::after` overlay (`--theme-overlay-image` / `-opacity` / `-blend`). The avatar window does NOT add it (must stay transparent).

---

## Canvas-backed theme effects (`MatrixRain`, `CyberParticles`)

`matrix` and `cyber` add a moving `<canvas>` background via two self-contained components: `components/MatrixRain.vue` (green digital rain) and `components/CyberParticles.vue` (drifting particles + connection lines + pulse bursts). They are mounted in all 7 non-avatar windows as the first children of the `.theme-bg` root, and **never** in the avatar window (`App.vue` / `App.mock.vue`), which must stay transparent.

### Contract (must hold for every canvas-backed theme component)

- **Theme-gated render**: `v-if="isMatrix"` / `v-if="isCyber"` (computed off `config.styleTheme`). When the theme is inactive the canvas is not in the DOM and costs zero CPU.
- **Layering**: `position:absolute; inset:0; z-index:-1; pointer-events:none`, sitting behind content inside the `.theme-bg` stacking context (`.theme-bg` is `position:relative; z-index:0`).
- **RAF lifecycle**: throttle to ~24–30 fps via timestamp; `tick()` must early-return and zero `rafId` when the theme is inactive or `document.hidden`. Guard every (re)start with `if (!rafId)` to avoid double loops.
- **Pause when hidden**: listen `visibilitychange`; `stop()` on hide, resume on show (only if `!rafId`).
- **Cleanup on unmount** (no leaks): `onBeforeUnmount` must `removeEventListener('visibilitychange')`, `cancelAnimationFrame`, `ResizeObserver.disconnect()`, and null the 2D context.
- **Switch off the theme**: the `watch(isActive)` handler must `stop()` + clear the canvas when turning off, and re-`start()` (after a RAF so `v-if` has mounted the canvas) when turning on.
- **Hardcoded colors are allowed here**: these are fixed visual effects (matrix green, cyber blue/purple), orthogonal to the token palette — like the other allowed exceptions above. The surrounding UI still uses tokens.

A future canvas-based theme should follow the same contract.
