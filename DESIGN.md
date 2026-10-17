# Design

## Visual theme

Neutral, closely-related tonal surfaces with a single restrained blue accent. Depth comes from tonal steps (window → sidebar → content → elevated), not borders or shadows. Dark theme uses warm-neutral dark grays, never pure black; light theme uses off-whites with a slightly recessed field tone.

## Color tokens (`theme::Palette`)

Surfaces: `bg_window` (chrome), `bg_sidebar`, `bg_content`, `bg_elevated` (popovers/dialogs/active tabs), `field_inset` (inputs), `stripe` (alternating rows), `surface_hover` / `surface_active` / `surface_selected`.

Text: `text_primary`, `text_secondary`, `text_muted`, plus `accent`, `danger`, `warn`. `accent_solid` is the opaque primary-action fill with white labels; it is deliberately darker than the focus/icon accent.

Interaction: `accent` (solid, primary actions/selection edge), `accent_fill` (translucent tint for selections/focused chrome), `border_soft` (functional hairlines only).

## Typography

One UI family (system sans; user-selectable bundled/system fonts) + monospace for terminal/code. Shared logical-point metrics: page title 22pt, section heading 16pt, body 14pt, secondary 12.5pt. Hierarchy through size and foreground contrast, not uppercase microtype. Some specialized controls retain local type sizes.

## Spacing scale

4 / 8 / 12 / 16 / 20-24 / 32 logical px. Settings rows use 12-16pt between items, 24-32pt between sections.

## Shape & elevation

Corner radii: controls 6-7, menus/popovers 10, windows/cards 12. Soft single drop shadows on floating surfaces only. No stacked borders, no glow.

## Components

- Buttons: `primary` (accent fill, white text), `secondary` (surface fill), `ghost` (transparent until hover), `destructive` (danger fill/text).
- Inputs: recessed `field_inset` fill, 6px radius, accent focus edge.
- Menus: elevated surface, 10px radius, 30px rows, leading icon/check column, right-aligned shortcuts, painted submenu arrow, soft separators between groups only.
- Controls: 34pt standard buttons, 32pt segmented controls and tabs, 40pt settings navigation. File density row heights: 24 / 30 / 36pt.
- Settings page: left category nav (icon + label, selected pill), adjacent readable content column (max 840pt), label+description left / control right rows, stacking below 620pt of content width. The global Places sidebar is suppressed on workspace tabs.
- Bulk Rename: header + 340pt rule column + virtualized comparison capped at 840pt + sticky footer with validation summary. Below 920pt, rules use a disclosure instead of a permanent column.
- Gallery: lazily created 256px thumbnail cache, capped at 64 entries (16 MiB of RGBA texels); Details/Grid retain the 64px cache. Non-image glyphs do not expand to photo-thumbnail size.
- Empty states: centered painted glyph + short message + one contextual action.
