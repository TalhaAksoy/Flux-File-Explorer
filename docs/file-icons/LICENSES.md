# Flux file icon artwork and attribution

The vector geometry in `crates/flux-ui/src/file_icons.rs` is original artwork
created for Flux Files. It is distributed under the project's declared
`MIT OR Apache-2.0` license. The registry is independently maintained source
code, not a copied third-party icon pack.

No third-party icon artwork, downloaded brand-logo files, SVG collection,
icon font, raster sprite sheet, or remote asset service is included.
The artwork uses geometric format/category symbols and descriptive language
abbreviations. Product names identify compatible file formats, not endorsement.
Trademarks remain the property of their respective owners; these are not
claimed to be official product logos.

Existing egui/UI typefaces remain governed by their existing licenses; see
`crates/flux-ui/src/fonts/LICENSES.md`. File icon letters use the user's current
UI font, not a newly redistributed logo/font package.

Native application/document artwork is obtained at runtime through the
existing OS provider on Windows/macOS. Such artwork is not bundled or relicensed
by Flux. Native thumbnails represent user files and are likewise not shipped
as icon assets. Linux always has the original offline Flux vector fallback.

## Artwork pipeline

- Registry: `flux-core/src/file_type.rs`, one stable `IconId` per identity.
- Drawing: normalized 24-unit coordinates; reusable line, arc, ellipse,
  polygon and typography primitives. No per-row SVG parsing or image decoding.
- Density/DPI: egui tessellates at the current scale; native raster requests
  use bounded physical-size buckets rather than enlarging one 32px image.
- Bundled artwork has no per-file GPU texture allocation. Native artwork is
  shared by identity/extension except actual applications/bundles, which use
  path-specific keys. Native caches are bounded and LRU-evicted.

Adding a format normally changes registry data and tests. Adding a genuinely
new motif also changes `file_icons.rs`; copying third-party artwork requires a
separate license review and an explicit attribution entry here.
