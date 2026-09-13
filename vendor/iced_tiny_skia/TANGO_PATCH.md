# Local software canvas clipping fix

This is the published `iced_tiny_skia` 0.14.0 source, from Iced commit
`3997291f318a8bc06fa522f5579836fb3feb94df` (`tiny_skia/`), with its published
Cargo manifest and the upstream MIT license. Dependencies and features are
unchanged.

`Layer::draw_primitive_group` and `draw_primitive_cache` already transform
geometry clip bounds into renderer coordinates. Drawing must not apply that
transformation a second time. Cached damage bounds likewise already carry it.
The two changes are in `src/lib.rs` and `src/layer.rs`.

The regression test lives in
`tango-script-iced/tests/presentation.rs::translated_canvas_clips_geometry_once_including_cached_geometry`.
It checks every pixel of a translated, clipped canvas against its expected
bounds, for both uncached and cached geometry and both themes. It does not use
another canvas as the expected image, since that would conceal the same bug.

Remove this patch when an upstream release includes equivalent fixes, retaining
the regression test.
