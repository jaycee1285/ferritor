# Design Basics

This document is a baseline design manifesto for apps in the digtwin ecosystem. It is meant to reduce avoidable design drift across stacks and targets, not to encourage novelty for its own sake.

The core position is simple: do not invent visual systems from scratch when better-tested ones already exist. Start from proven theme sources, codify the mappings, and apply them consistently.

## Core Principles

1. Theme from proven systems, not from improvisation.
2. Use one clear source of truth per target so color bugs are easy to trace and smoke test.
3. Prefer consistency across apps over clever one-off styling.
4. Optimize for low cognitive load, fast scanning, and strong state visibility.
5. Design for the actual interaction model of the device, not an abstract cross-platform ideal.
6. Progressive disclosure beats permanently visible chrome.

## Color System

### Primary Rule

All color work starts from terminal themes, specifically the Base16-style YAML themes in `~/.local/share/themes`.

The reasoning is practical:

- Terminal themes are already heavily pressure-tested by demanding users.
- They produce coherent palettes faster than hand-rolled app palettes.
- They create a common starting point across web, desktop GUI, syntax highlighting, and TUI work.
- A shared origin makes regressions and mismatches easier to debug.

### Target Mapping

| Target | Source of truth | Implementation guidance | Notes |
| --- | --- | --- | --- |
| Web projects | Base16 YAML themes | Follow the DayLight `yamltoskeleton.ts` pipeline and map into Skeleton-style tokens | Default stack is usually Astro or Svelte + Skeleton |
| Mobile apps | Same Base16-derived scheme as web | Reuse the same generated CSS/theme structure used by DayLight | Even in one-off apps, color variables should follow Skeleton token naming |
| Desktop GUI apps | GTK color variables | Derive from GTK-style variables even when the app does not use GTK directly | Consistent GTK variable naming makes state and surface bugs easier to test |
| Syntax highlighting | `.tmTheme` files generated from the same YAML ecosystem | Use for syntax across Rust apps via Syntect and web apps via Shiki | Syntax colors should stay aligned with the active theme family |
| TUI apps | Kitty theme semantics | Treat background, foreground, cursor, selection, and the 16 core colors as the baseline | Kitty themes are a useful operational reference even when implementation differs |

### Supporting Tooling

| Tool or source | Role |
| --- | --- |
| `~/.local/share/themes` | Main library of Base16 YAML, `.yml`, and related theme files worth considering |
| DayLight `yamltoskeleton.ts` | Current model for web theme conversion |
| `~/repos/base16changer` | Useful reference for mapping Base16 themes into GTK and terminal-friendly variables |
| `~/.config/gtk-4.0/gtk.css` | Local GTK variable output and practical smoke-test target |

### Color Rules

1. Do not start by picking brand colors manually unless there is a documented reason.
2. Derive app colors from the theme source first, then adjust only where product constraints require it.
3. State colors must be explicit and consistent: default, hover, active, selected, disabled, dirty, destructive, success, warning.
4. Hover and selection must never collapse into the same appearance.
5. Surface distinctions must remain legible: sidebar, editor, panel, modal, ribbon, status area, and content canvas should not blur together.
6. Syntax colors are part of the product experience, not an afterthought.

## Typography

Typography should be simple and consistent across projects:

- Use a bold monospace face for headlines, titlebars, button text, and strong labels.
- Use the matching sans face in regular weight for body text, menus, and general UI content.
- Do not overthink pairing unless the product has a specific reason to deviate.

### Preferred Pairings

| Monospace | Sans | Character |
| --- | --- | --- |
| Spline Sans Mono | Spline Sans | Current default desktop pairing |
| Iosevka Charon Mono | Iosevka Charon | More technical and sharper |
| B612 Mono | B612 | Highest readability; cockpit-derived |
| Fragment Mono | Work Sans | Most humanistic option in this set |

### Typography Rules

1. Default to one pairing per product and use it everywhere.
2. Monospace is for emphasis and structure, not for long-form body copy.
3. Sans is for reading comfort and dense UI.
4. This rule applies to websites, portfolios, desktop apps, mobile apps, and utilities alike.

## Spatial System

The spacing system should not default to a generic 8-point ladder. The goal here is a more human-feeling rhythm that still remains easy to calculate, easy to memorize, and easy to audit.

The working model is:

- Use a 4-based underlying grid for alignment and snapping.
- Build the primary spacing scale from Fibonacci-style multiples.
- Tie spacing to target and density instead of pretending one ladder works equally well everywhere.
- Give agents a constrained token set rather than open-ended pixel freedom.

### Why This Exists

The usual 8-point system is easy to teach but tends to flatten design into a predictable and over-regular cadence. A raw 4-point system is more flexible, but it increases choice count and makes consistency harder to hold in working memory.

The intended compromise is a Fibonacci-like scale running on top of a 4-aligned base. That keeps the system calculable while producing layouts that feel less mechanical.

### Core Rules

1. Alignment snaps to 4-based values.
2. Spacing tokens come from Fibonacci-derived tiers, not arbitrary increments.
3. Each target gets a base unit, then reuses the same tier logic.
4. Components should use named spacing tiers, not hand-picked pixel values.
5. Desktop may default to larger tiers, but the smallest tier is still available where tight grouping is needed.

### Spacing Tiers

| Token | Multiplier | Purpose |
| --- | --- | --- |
| `space-1` | 1x | Tight related elements |
| `space-2` | 2x | Default intra-group spacing |
| `space-3` | 3x | Comfortable component spacing |
| `space-5` | 5x | Section and panel separation |
| `space-8` | 8x | Major route, hero, or layout separation |

### Default Base Units

| Target | Base unit | Notes |
| --- | --- | --- |
| Mobile | 12px | Derived from `4 x 3`; works with the preferred mobile text baseline |
| Desktop and TUI | 8px | Still snaps cleanly to 4 while keeping desktop layouts practical |
| Large-format or marketing surfaces | Contextual | May scale upward when the smallest text or interaction target is substantially larger |

### Example Token Values

| Token | Mobile @ 12px BU | Desktop/TUI @ 8px BU |
| --- | --- | --- |
| `space-1` | 12px | 8px |
| `space-2` | 24px | 16px |
| `space-3` | 36px | 24px |
| `space-5` | 60px | 40px |
| `space-8` | 96px | 64px |

### Usage Guidance

| Situation | Preferred token |
| --- | --- |
| Label and related control | `space-1` |
| Elements within the same card or form group | `space-2` |
| Internal component padding or stacked control groups | `space-3` |
| Separation between panels, cards, or major sections | `space-5` |
| Major page bands, hero spacing, route-level separation | `space-8` |

### Sizing Guidance

This system is not only for gaps. It should also influence component sizing and spatial rhythm.

- Button heights should generally land around `3x` to `5x` the base unit depending on density and platform.
- Card, modal, and panel padding should usually start at `space-2` or `space-3`.
- Large desktop boxes and panels should scale through Fibonacci-related divisions rather than evenly repeated halves.
- Target measurements should snap down to the nearest 4-aligned value when implementation requires a clean pixel result.

### Operational Guidance for Agents

When building layouts:

1. Choose the target first.
2. Set the base unit for that target.
3. Pick spacing from the named tier table.
4. Snap final implementation values to 4-based alignment where needed.
5. Escalate to larger tiers to communicate hierarchy, not just to add empty air.

## Layout: Desktop and TUI

Desktop and TUI interfaces should respect keyboard-heavy usage without copying Vim or Emacs mental models.

### Interaction Rules

1. Prefer conventional shortcut language such as `Ctrl+X`, `Ctrl+C`, and `Ctrl+V`. Avoid modal-editor terminology and patterns.
2. Roughly 90-95% of daily-used actions should be reachable from one menu, one ribbon or command area, and shortcuts.
3. Desktop layouts should not mimic the mobile version. Different access patterns justify different structures.
4. Use progressive disclosure. Keep the default interface focused and move less-used controls behind search, tabs, menus, or `Ctrl+H`-style help.
5. Every primary work surface should be maximizable by collapsing adjacent panes or secondary regions.

### Readability and Density

| Element | Minimum size |
| --- | --- |
| Menu items | 16px |
| Other UI text | 13px |

These are minimums, not targets to optimize downward.

### State and Surfaces

1. State must always be visible.
2. Unsaved documents must read as dirty.
3. Hover, focus, selected, active, and disabled states must all be distinguishable.
4. Surface boundaries should help concentration, not fragment it.
5. Spacing, padding, and alignment rules should be codified in the CIE system and then followed consistently across components and dialogs.

## Layout: Mobile

Mobile design should optimize for thumb reach, glanceability, and accidental-tap prevention.

### Core Rules

1. Respect the status bar and navigation area on every screen.
2. The top third is the primary viewing area.
3. The bottom third, especially the bottom-right area, is the primary interaction zone.
4. Each route or screen should have one primary action.
5. Secondary actions can expand upward from a secondary FAB or related control.
6. Do not use side sheets as a lazy default. Only use them when the feature genuinely needs sustained secondary workspace.
7. Dialogs and destructive confirmations should be centered vertically to reduce accidental taps while preserving comfortable reach.
8. Apply the same state-visibility and spacing discipline used on desktop.

### Mobile Anti-Patterns

| Avoid | Why |
| --- | --- |
| Ignoring safe areas | Creates immediate visual and interaction bugs |
| Top-heavy action placement | Fights thumb ergonomics |
| Multiple competing primary actions | Increases cognitive load and hesitation |
| Casual side-sheet usage | Usually wastes space and weakens visual discipline |
| Weak state indication | Makes touch interfaces feel ambiguous and error-prone |

## Default Design Decisions by Stack

| Stack or app type | Default design decision |
| --- | --- |
| Astro or Svelte web app | Use Skeleton-style tokens generated from Base16 YAML themes |
| Svelte + Tauri mobile or desktop | Reuse the DayLight theme generation approach and Skeleton token naming |
| GTK or native desktop GUI | Normalize around GTK variable semantics even outside GTK |
| Rust or web code editor surfaces | Use theme-aligned `.tmTheme` syntax colors through Syntect or Shiki |
| TUI | Start from Kitty theme semantics and preserve strong state contrast |

## Agent Guidance

When designing or implementing UI in this ecosystem:

1. Identify the target first: web, mobile, desktop GUI, syntax layer, or TUI.
2. Find the correct theme source before choosing colors.
3. Reuse existing conversion pipelines where they already exist.
4. Name variables according to the dominant system for that target, especially Skeleton tokens or GTK variables.
5. Verify state coverage explicitly before considering a design complete.
6. Keep interfaces focused; add capability through disclosure, not permanent chrome.

## Future Work

- Define the equivalent Base16-to-ShadCN mapping.
- Codify spacing, radius, and density rules in the CIE system.
- Add explicit token examples for Skeleton, GTK, and Kitty-derived themes.
- Add a compact smoke-test checklist for theme regressions and state visibility.
