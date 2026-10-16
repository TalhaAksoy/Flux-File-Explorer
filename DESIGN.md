# Design

## Visual theme

Neutral, closely-related tonal surfaces with a single restrained blue accent. Depth comes from tonal steps (window → sidebar → content → elevated), not borders or shadows. Dark theme uses warm-neutral dark grays, never pure black; light theme uses off-whites with a slightly recessed field tone.

## Color tokens (`theme::Palette`)

Surfaces: `bg_window` (chrome), `bg_sidebar`, `bg_content`, `bg_elevated` (popovers/dialogs/active tabs), `field_inset` (inputs), `stripe` (alternating rows), `surface_hover` / `surface_active` / `surface_selected`.

Text: `text_primary`, `text_secondary`, `text_muted`, plus `accent`, `danger`, `warn`.

Interaction: `accent` (solid, primary actions/selection edge), `accent_fill` (translucent tint for selections/focused chrome), `border_soft` (functional hairlines only).

## Typography

One UI family (system sans; user-selectable bundled/system fonts) + monospace for terminal/code. Scale: page title ~22pt, section heading ~15pt, body/controls 13.5pt, secondary 11.5-12pt, fine metadata 11.5pt. Hierarchy through size + muted color, not uppercase microtype.

## Spacing scale

4 / 8 / 12 / 16 / 20-24 / 32 logical px. Settings rows use 12-16pt between items, 24-32pt between sections.

## Shape & elevation

Corner radii: controls 6-7, menus/popovers 10, windows/cards 12. Soft single drop shadows on floating surfaces only. No stacked borders, no glow.

## Components

- Buttons: `primary` (accent fill, white text), `secondary` (surface fill), `ghost` (transparent until hover), `destructive` (danger fill/text).
- Inputs: recessed `field_inset` fill, 6px radius, accent focus edge.
- Menus: elevated surface, 10px radius, 30px rows, leading icon/check column, right-aligned shortcuts, painted submenu arrow, soft separators between groups only.
- Settings page: left category nav (icon + label, selected pill), centered content column (max ~880px), label+description left / control right rows.
- Bulk Rename: header + left rule column (~360-420px) + right virtualized before/after preview + sticky footer with validation summary.
- Empty states: centered painted glyph + short message + one contextual action.
