# Comprehensive native file icon system

Approved: original vector motifs plus existing asynchronous native icon provider; no browser validation. Start from master 0be3d65, not the separate premium UI branch.

## Architecture and rules

One GUI-independent, data-driven registry in flux-core owns icon identity and human-readable type labels. Resolve filesystem kind, application bundles, exact filenames, path/filename patterns, longest compound suffix, ordinary suffix, optional reliable cached evidence, then generic fallback. ASCII matching operates on OsStr encoded bytes without changing the real filesystem path. Directories/symlinks remain special objects. Native application artwork is preferred in Automatic/System when available; Flux mode remains coherent. Never inspect private keys or configuration secrets for artwork.

Original vector geometry in flux-ui shares a consistent optical box and stroke language. Language/tool motifs, document families, feeds, media, archives and specialized data are not simply differently colored generic sheets. Geometry is drawn at requested logical size, avoiding runtime SVG parsing and raster enlargement. Shared IDs mean duplicate filenames do not allocate duplicate bundled textures.

Evolve IconCache: bounded native texture bytes/count, bounded requests and result uploads, LRU rather than wholesale reset, size buckets and theme keys, negative caching, metrics. A separate bounded evidence queue/cache inspects at most 4096 bytes for ambiguous .ts/.m and XML/JSON feed candidates, on a worker only. Reject XML DTD/entity declarations; no parser expansion, execution, or network fetch. Cache evidence per path with explicit invalidation on directory refresh/navigation and a bounded lifetime. Unknown/unreadable/malformed input retains the deterministic filename classification.

Persist Flux/System/Automatic in settings with backward-compatible defaults. Preference changes do not flush unrelated thumbnail/native resources. Use the same drawing/classification route in Details, Compact, Grid, Gallery, Search, Preview, Properties and drag feedback. Preserve thumbnail precedence.

## Validation / implementation plan

1. Record baseline build, icon rendering and representative 1k/10k/100k workload measurements.
2. Registry and table-driven tests covering every requested group, ambiguous names, compound suffixes and non-UTF-8 names.
3. Original vector artwork, license documentation, shared identity presentation.
4. Bounded asynchronous caches/evidence, source preference and all-view integration.
5. Real native screenshot iterations in both themes and multiple sizes/DPI; profiling and cache tests.
6. fmt, strict all-target clippy, workspace tests, release, existing benchmarks and cross-platform CI. PR/master integration only after review and successful verification. Report unavailable platform runtime checks and missing measurements explicitly.

No filesystem engine rewrite, no third-party artwork downloads, no new heavyweight dependencies, no synchronous UI-thread filesystem reads.
