# BN4 native save references

The twelve raw saves in `../templates` are the native Red Sun/Blue Moon,
US/Japanese templates: Dark HP 997, Light HP 999 and Light HP 1000.

`references.luau` records CRC32 values produced with the existing native save
implementation. Each template has eight additional cases: shifts 0, 4, 252 and
508, each with the first save byte set to 0 or 1. The zero-byte cases exercise
the region-ambiguous checksum format. Both the shifted input and native
normalized output are checked. The package additionally preserves surrounding
SRAM that the native exporter zeroes.

Each `*-folders.bin` contains 45 native folder operations, acceptance results,
byte deltas, getter snapshots and a final encoded-save CRC32. The test applies
the deltas to the previous expected buffer and compares every byte after each
operation. Coverage includes all three folders, Regular selections and the
equipped-folder cache, chip/code slots, clearing, pack counts, anticheat mirrors
and rejected indices. Native positions are converted to Lua's 1-based positions
only in the test adapter.

Each `*-model.bin` contains 269 native NaviCust, Mod Card and Auto Battle Data
operations in the `bn4-model-v2` format. It records acceptance, byte deltas,
getter/derived-stat snapshots, Mod Card violations and a final encoded CRC32.
The 3,228 total mutations cover every placement slot, materialization, color
bars, fixed enabled/disabled card slots, anticheat, use counts and ranking.
HP and folder-limit cases include duplicate grid cells, command-line placement,
integer wrapping and caps. Invalid and disabled card bindings are checked
separately from their effective stats.

The materialization oracle uses synthetic bitmaps, including compressed
fallback cases. Actual BN4 ROM assets expose no compressed bitmaps, matching
the native reader. Its color encoding also differs from BN5: pink precedes yellow.

`catalog.luau` captures native public asset APIs using a synthetic ROM. It
contains all 266 regional Mod Card entries (name, slot, typed effect and bugs),
188 NaviCust effect lists and ten bug groups. The Luau comparisons normalize
case/underscores only for native enum names; card names and payloads match exactly.

References were generated from the current native BN4 data-view implementation
using the local oracle at
`/tmp/tango-bn4-package-parity/src/bin/{codec,folders,model,catalog}.rs`.
No game ROM is needed. Run the checked-in comparisons with:

```sh
cargo test --release --locked -p tango-script --test packages bundled_
```

The complete suite runs each template in English and Japanese in a fresh
callback, retaining normal runtime limits. `bn4.test()` checks one template by
default; the harness selects others through the `test.fixture` immutable input.
