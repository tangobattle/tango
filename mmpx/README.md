# mmpx-rs

[![Crates.io](https://img.shields.io/crates/v/mmpx.svg)](https://crates.io/crates/mmpx)

This is a Rust version of the
[MMPX image magnifying algorithm](https://casual-effects.com/research/McGuire2021PixelArt/index.html)
as presented by Morgan McGuire and Mara Gagiu.

All rights to this algorithm belong to the authors,
and their choice of the MIT license applies to this implementation as well (see LICENSE.md).

## algorithm

The algorithm is designed for scaling up pixel art 2x by testing various rules against the
[neighborhood](https://casual-effects.com/research/McGuire2021PixelArt/8.png)
of each source pixel to map the 4 outcome pixels.
It borrows and builds on some concepts from nearest neighbor and EPX
(other common methods for pixel art scaling).

The algorithm offers modular neighborhood based rules that operate sequentially,
enabling some future work to potentially add more rules under this same multithreaded processing "environment".

A key assumption in these rules is that nearby pixels have the same RGB value,
meaning non pixel art style images will likely fall back to nearest neighbor.

## cli
```sh
$ cargo install mmpx
$ mmpx input.png --output output.png
```

## library
##### **`Cargo.toml`**
```toml
[dependencies.mmpx]
version = "0.1.0"
```

##### **`main.rs`**
```rust
use mmpx;
use image::RgbaImage;
.
.
.
// Load using image
let image: RgbaImage = image::open(input_path).unwrap().to_rgba8();
// Magnify 2x
let output: RgbaImage = mmpx::magnify(&image);
```

## implementation

This implementation uses Rust for memory safety and the
[rayon](https://github.com/rayon-rs/rayon)
crate to process pixels in parallel.
The only processing logic that intentionally differs from the C++ version offered by the authors
is in the calculation of luma for a given pixel (a minor effect, if any).

Performance seems similar to the single threaded C++ implementation, making this potentially usable
for realtime rendering. Testing has been minimal and improvements could come from using a dry image buffer
like the c++ version instead of the
[image](https://github.com/image-rs/image)
crate's `ImageBuffer`.
