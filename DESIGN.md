---
name: FerrisTrace
description: Self-hosted observability. Find what broke, fix it, go back to bed.
colors:
  primary: "#4f46e5"
  primary-hover: "#4338ca"
  primary-foreground: "#ffffff"
  accent-text: "#4338ca"
  accent-subtle: "#eef2ff"
  series: "#4f46e5"
  background: "#ffffff"
  sidebar: "#fafafa"
  card: "#ffffff"
  muted: "#f4f4f5"
  row-hover: "#f6f6f7"
  foreground: "#09090b"
  muted-foreground: "#52525b"
  faint-foreground: "#71717a"
  border: "#e4e4e7"
  border-subtle: "#efeff1"
  input: "#d4d4d8"
  destructive: "#dc2626"
  destructive-text: "#b91c1c"
  destructive-subtle: "#fef2f2"
  warning-text: "#92400e"
  warning-subtle: "#fffbeb"
  success-text: "#15803d"
  success-subtle: "#f0fdf4"
  status-ok: "#16a34a"
  status-degraded: "#d97706"
  status-outage: "#dc2626"
  dark-background: "#0c0c0e"
  dark-card: "#111114"
  dark-foreground: "#fafafa"
  dark-primary: "#8b93ff"
typography:
  title:
    fontFamily: "Source Sans 3 Variable, ui-sans-serif, system-ui, sans-serif"
    fontSize: "20px"
    fontWeight: 700
    lineHeight: "28px"
    letterSpacing: "-0.005em"
  heading:
    fontFamily: "Source Sans 3 Variable, ui-sans-serif, system-ui, sans-serif"
    fontSize: "16px"
    fontWeight: 700
    lineHeight: "24px"
  sub:
    fontFamily: "Source Sans 3 Variable, ui-sans-serif, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 700
    lineHeight: "20px"
  body:
    fontFamily: "Source Sans 3 Variable, ui-sans-serif, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: "20px"
  control:
    fontFamily: "Source Sans 3 Variable, ui-sans-serif, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 600
    lineHeight: "18px"
  caption:
    fontFamily: "Source Sans 3 Variable, ui-sans-serif, system-ui, sans-serif"
    fontSize: "12px"
    lineHeight: "16px"
  micro:
    fontFamily: "Source Sans 3 Variable, ui-sans-serif, system-ui, sans-serif"
    fontSize: "11px"
    lineHeight: "16px"
  mono:
    fontFamily: "JetBrains Mono Variable, ui-monospace, Menlo, monospace"
    fontSize: "12px"
    fontWeight: 500
    fontFeature: "tnum"
rounded:
  sm: "4px"
  md: "6px"
  lg: "8px"
  xl: "12px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "24px"
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.primary-foreground}"
    typography: "{typography.control}"
    rounded: "{rounded.md}"
    padding: "0 12px"
    height: "32px"
  button-primary-hover:
    backgroundColor: "{colors.primary-hover}"
  button-outline:
    backgroundColor: "{colors.background}"
    textColor: "{colors.foreground}"
    typography: "{typography.control}"
    rounded: "{rounded.md}"
    padding: "0 12px"
    height: "32px"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.foreground}"
    rounded: "{rounded.md}"
    height: "32px"
  input:
    backgroundColor: "{colors.background}"
    textColor: "{colors.foreground}"
    typography: "{typography.control}"
    rounded: "{rounded.md}"
    padding: "0 10px"
    height: "32px"
  card:
    backgroundColor: "{colors.card}"
    rounded: "{rounded.lg}"
  badge-destructive:
    backgroundColor: "{colors.destructive-subtle}"
    textColor: "{colors.destructive-text}"
    rounded: "{rounded.sm}"
    height: "20px"
  badge-success:
    backgroundColor: "{colors.success-subtle}"
    textColor: "{colors.success-text}"
    rounded: "{rounded.sm}"
    height: "20px"
---

# Design System: FerrisTrace

## Overview

**Creative North Star: "The Calm Control Room"**

FerrisTrace is the room a team walks into when production misbehaves. Everything is visible at a glance and nothing raises its voice except what is broken. The neutral field is quiet zinc, the accent is a single indigo used for action and for the data mark, and colour beyond that is reserved for state: red for broken, amber for degraded, green only where "fine" is the answer someone asked for.

The system is dense but ordered. Type sits small (14 px body, 13 px controls) on a strict scale, numbers are tabular and often monospaced, and surfaces are flat panels separated by hairline borders. The same identity runs on ferristrace.fr; the app refines it and does not depart from it.

Two people use this room with equal weight: the developer paged at night on a phone, and the SRE who lives in it all day on a wide screen. Each screen must be legible to the first and fast for the second.

**Key Characteristics:**
- Quiet neutrals, one accent, colour reserved for state.
- Flat surfaces, hairline borders, shadows only on floating layers.
- Small, strict type scale; tabular numbers; monospace for identifiers and values.
- Light and dark themes, both first-class.
- Problems rank first and carry the only loud colour on the screen.

## Colors

A zinc neutral field with one indigo voice and a strict state vocabulary.

### Primary
- **Signal Indigo** (`primary`): primary buttons, the focus ring, links (`accent-text`), the selected state (`accent-subtle`). In dark mode it lifts to a softer periwinkle (`dark-primary`) so it keeps contrast on near-black.
- **Data Indigo** (`series`): the fill of a chart mark. Its own token so charts never repaint when the accent changes.

### Neutral
- **Paper White / Night Ink** (`background`, `dark-background`): the page.
- **Sidebar Haze** (`sidebar`): the navigation rail, one step off the page.
- **Zinc Wash** (`muted`, `row-hover`): hover rows, secondary buttons, segmented controls.
- **Ink, Graphite, Ash** (`foreground`, `muted-foreground`, `faint-foreground`): primary text, secondary text, placeholders and tertiary labels.
- **Hairline** (`border`, `border-subtle`, `input`): every separation in the system.

### State
- **Broken** (`destructive`, `destructive-text`, `destructive-subtle`), **Degraded** (`warning-*`), **Healthy** (`success-*`), and the status dots (`status-ok`, `status-degraded`, `status-outage`).
- Log severities (trace, debug, info, warn, error, fatal) and eight service hues (`--svc-1` to `--svc-8`) each carry a foreground, background and border triple per theme; see the sidecar.

### Named Rules
**The Only Loud Thing Rule.** Red and amber appear only where something is broken or degrading. If nothing is wrong, the screen has no saturated colour besides the indigo accent.

**The Green Is An Answer Rule.** Green marks health only where the user asked "is it fine?" (a status, a check). Rows that are simply running stay neutral.

## Typography

**Body Font:** Source Sans 3 Variable (with ui-sans-serif, system-ui)
**Mono Font:** JetBrains Mono Variable (with ui-monospace, Menlo)

**Character:** A legible humanist sans for reading under stress, paired with a precise mono for everything a machine produced: ids, durations, timestamps, values.

### Hierarchy
- **Title** (700, 20/28px): the page heading, one per page.
- **Heading** (700, 16/24px): section and sheet titles.
- **Sub** (700, 14/20px): card titles and group labels.
- **Body** (400, 14/20px): running text and table cells.
- **Control** (600, 13/18px): buttons, inputs, tabs, segmented controls.
- **Caption** (12/16px) and **Micro** (11/16px): metadata, hints, counts, axis labels.
- **Mono** (500, 11.5–12.5px, tabular): trace ids, durations, values, code.

### Named Rules
**The Machine Wrote It Rule.** Anything a system produced (ids, hashes, durations, timestamps, query text) is set in mono with tabular figures; anything a person reads as prose is set in the sans.

## Layout

A fixed navigation rail on the left (desktop) and a five-slot bottom tab bar on phones (`md` breakpoint, 768px), with a sheet for the full menu. Content pages open with a page header (title, description, actions), then filters, then the data. Spacing follows a 4px grid (4, 8, 12, 16, 24). Tables and lists are full-width; summary tiles sit in responsive grids above them. On phones, filters collapse behind a single "Filters" toggle and touch targets grow to 44px or more (52px for tabs).

## Elevation & Depth

Flat by default. Surfaces are separated by hairline borders and one-step tonal shifts (page, sidebar, card, muted), never by shadow. Shadows exist only for layers that float above the page: popovers, menus, dialogs and sheets (`elevation-md`, `elevation-lg`), over a dimming overlay for modal layers.

### Shadow Vocabulary
- **Hairline lift** (`elevation-sm`): outline buttons and inputs only, a 1px hint that the control is pressable.
- **Floating** (`elevation-md`): popovers, menus, the command palette.
- **Modal** (`elevation-lg`): dialogs and sheets.

### Named Rules
**The Flat Room Rule.** A card at rest has a border and no shadow. Depth means "this floats above the page", nothing else.

## Shapes

Gently rounded and consistent: 4px for badges and chips, 6px for buttons and inputs, 8px for cards and panels, 12px for dialogs, and a 16px top radius for mobile sheets. Status is often a small square or dot rather than a pill, so many items line up on a grid (node cards with one square per pod).

## Components

### Buttons
- **Shape:** gently rounded (6px), 32px tall by default (28px small, 40px large).
- **Primary:** Signal Indigo fill, white label, one per view for the main action.
- **Outline (default):** page background, `input` border, hairline lift; the everyday button.
- **Secondary / Ghost:** zinc wash or transparent, for toolbars and dense rows.
- **Destructive:** red fill, only to confirm a destructive action.
- **Focus:** 2px indigo outline, 1px offset. Loading swaps in a spinner and sets `aria-busy`.

### Badges
- **Style:** 20px tall, 4px radius, subtle fill plus matching border per state (neutral, outline, accent, success, warning, destructive). A mono variant carries values and versions.

### Cards / Containers
- **Corner Style:** 8px.
- **Background:** `card`, with a hairline border and no shadow.
- **Header:** title row with a bottom border, 16px horizontal and 12px vertical padding.

### Inputs / Fields
- **Style:** 32px, 6px radius, `input` border, hairline lift, placeholder in Ash.
- **Focus:** 2px indigo outline. **Error:** red border through `aria-invalid`. **Disabled:** zinc wash at 55% opacity.
- On touch devices inputs render at 16px so iOS does not zoom.

### Navigation
- **Desktop rail:** grouped items (label in micro uppercase), rows of 6×8px padding, a muted fill for the active page, counters for open incidents and firing alerts.
- **Phone tab bar:** Home, Incidents, Traces, Logs and Menu. The active tab has a 2px indigo bar on top. The menu opens a bottom sheet with every page in a two-column grid.

### Severity and Status Marks
Log severities render as badges in their own fg/bg/border triple; health renders as small squares or dots in status colours. These marks are how the Only Loud Thing Rule is implemented.

## Do's and Don'ts

### Do:
- **Do** rank problems first in every list and give them the only saturated colour on screen.
- **Do** set ids, durations, timestamps and values in JetBrains Mono with tabular figures.
- **Do** separate surfaces with hairline borders (`border`) and one-step tonal shifts.
- **Do** keep touch targets at 44px or more on phones, and inputs at 16px on touch devices.
- **Do** design both themes; check every state colour against `card` in light and dark.

### Don't:
- **Don't** put a shadow on a card at rest; shadows are for floating layers only.
- **Don't** paint healthy or merely running items green; neutral is the default for "nothing to see".
- **Don't** introduce a second accent hue in the app; the extra accents (teal, orange, rose, slate) belong to the public status page only.
- **Don't** use `primary` as a chart fill; use `series`.
- **Don't** present mock or missing data as real; say what is unknown.
