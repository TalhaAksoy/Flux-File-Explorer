# Flux Files native UI refinement

Approved direction: shared design-system and responsive workspace refinement, preserving Rust/egui, native decorations, filesystem semantics, unified tabs and virtualized views. No browser validation.

## Audit

The supplied Explorer, Rename and Settings screenshots expose small text, compressed controls, weak hierarchy and inefficient allocation of space. Existing strengths include semantic colors, shared widgets, asynchronous operations, worker-based preview planning and visible-row virtualization. Implementation must reconcile the existing DESIGN.md promises with actual layouts rather than create a second design system.

## Design

Neutral charcoal / neutral off-white surfaces; restrained blue selection and focus. Body 14pt, secondary 12.5pt, page title 22pt. Standard controls 32–34pt. Density remains independent of theme, with compact, standard and comfortable file rows. Semantic tokens and shared widget geometry are authoritative.

Explorer: comfortable navigation targets, bounded breadcrumbs, readable sidebar and tabs, consistent column headers and metadata. Grid reflow and preview behavior remain intact. Preview presents identity, visual content and grouped metadata without filesystem work in rendering. Secondary surfaces consume the same tokens.

Rename: 340pt rules column with responsive disclosure at narrow sizes; a bounded comparison region keeps original and proposed names close. Shared header/row column geometry. Full-height virtualized list reserves an actual persistent footer. Ready filenames use ordinary foreground rather than green body text; explicit status words retain semantics. Long values expose tooltips.

Settings: structured category navigation, an approximately 840pt content column aligned near navigation rather than floating far across the workspace. Comfortable label/description/control rows stack on narrow widths. Search, unique IDs, persistence and all preference semantics remain unchanged.

## Safety and validation

No file-engine rewrite, no new heavy dependencies, no per-frame filesystem work or continuous decorative repaint. Establish release benchmark baseline before edits. Compare identical datasets afterward, explicitly noting host memory pressure and warm-cache effects. Use native application screenshot harness and egui pointer-event tests, then formatting, clippy, workspace tests and release build. Cross-platform runtime verification is only reported if actually available. Do not merge with failing required checks or incomplete critical QA.

## Implementation sequence

1. Record baseline host, release benchmark and actual screenshots.
2. Shared type/spacing/contrast/control geometry, Explorer chrome, file views and preview.
3. Rename comparison geometry and persistent footer, Settings composition.
4. Native screenshots and correction loops; add targeted interaction/layout regression tests.
5. Required checks, matched performance rerun, report and Git delivery subject to verification.
