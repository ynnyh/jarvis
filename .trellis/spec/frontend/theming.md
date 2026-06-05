# Theming System (CSS Variable Tokens)

> How visual themes work and how to add/extend them safely.
> Source of truth: `desktop/src/style.css`. Composable: `desktop/src/composables/useTheme.ts`. Registry: `desktop/src/style-themes.ts`.

---

## Overview

The app ships 4 visual themes, switchable at runtime with **zero re-render**:

| id | name | character |
|----|------|-----------|
| `sci-fi` | 霓虹风 | magenta neon × deep-purple night (default) |
| `playful` | 俏皮风 | candy pastel, big radius, bouncy shadow |
| `zen` | 治愈风 | paper/wood, low-saturation beige |
| `minimal` | 极简风 | grayscale mono, hairline borders, no shadow/motion |

Theming is driven entirely by **CSS custom properties (design tokens)**. There is no JS color logic in components.

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

Real (non-avatar) windows add class `.theme-bg` on their root to get `--theme-bg` plus a `::after` overlay (`--theme-overlay-image` / `-opacity` / `-blend`). The avatar window does NOT add it (must stay transparent). A future canvas-based theme (e.g. matrix rain) should mount its canvas only on `.theme-bg` windows, behind content (`z-index:0; pointer-events:none`), never on the avatar window.
