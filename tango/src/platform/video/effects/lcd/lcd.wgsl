// GBA-style LCD pixel grid. Nearest-magnifies the native framebuffer (exactly
// like the pass-through) and draws a thin dark line along every native-pixel
// boundary, mimicking the visible inter-pixel gutter of the GBA's reflective
// LCD.
//
// The line is drawn in *screen space* using fragment-quad derivatives
// (`fwidth`), so it stays a constant ~1px wide no matter how far the widget is
// magnified to fit the window — crisp at every zoom, and it never beats against
// the pixel grid into a moiré the way a fixed-fraction gutter would. When a
// native pixel shrinks below a couple of screen pixels the lines naturally
// merge and the grid fades, instead of swallowing the picture.

// Brightness multiplier at the centre of a grid line (1.0 = no darkening).
const GRID_DARKNESS: f32 = 0.6;
// Grid line half-width, in screen pixels (the smoothstep falloff distance).
const LINE_WIDTH: f32 = 1.0;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(fb_texture));
    let coord = in.uv * dims; // position in native texel space

    // Nearest texel for the colour — crisp magnification, same as pass-through.
    let rgb = load(vec2<i32>(floor(coord)));

    // Signed distance (in texel units) to the nearest integer pixel boundary,
    // converted to screen pixels via the per-fragment derivative so the line
    // width is independent of how far the texture is magnified on screen.
    let dist = abs(fract(coord - 0.5) - 0.5) / fwidth(coord);
    let line = min(dist.x, dist.y); // nearest of the horizontal / vertical lines

    // 0 on a grid line, ramping to 1 just inside the pixel body.
    let inside = smoothstep(0.0, LINE_WIDTH, line);
    let shade = mix(GRID_DARKNESS, 1.0, inside);

    return vec4<f32>(rgb * shade, 1.0);
}
