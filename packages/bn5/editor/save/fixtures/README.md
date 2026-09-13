# BN5 save references

The eight `.raw` files are the existing native BN5 light/dark save templates,
copied without conversion. `references.luau` records the CRC32 of each complete
SRAM image produced by the native encoder after rebuilding its checksum.

Each `*-model.bin` contains 184 operations evaluated by the native BN5 dataview,
with every changed byte and a getter snapshot after each operation. The scripted
test applies the same operations and compares the whole save, including untouched
bytes outside the save region. Derived HP and folder limits are included.

These references use synthetic chips and 5×5 part shapes, with the formulas in
`../model_test.luau`, so no game ROM is needed. They cover both variants and regions,
light/dark karma, link Navi capability changes, both anti-tamper mirrors, clipping,
rotation, overlapping/compressed parts, patch-card enablement, Auto Battle Data
ranking and empty slots, first/last collection positions, and rejected mutations.

The binary format uses little-endian u32 numbers and UTF-8 strings prefixed with
their byte length. It contains the magic `bn5-model-v1`, template name and operation
count, followed by each operation's name, argument count/arguments, accepted flag,
changed-byte count/(WRAM offset, byte) pairs, and snapshot length/values. Native
collection positions are converted to 1-based positions at the test boundary.
`0xffffffff` represents an absent snapshot field. The test reads all bytes and
all snapshot fields; it does not derive expected results from the Luau model.

The native generator used during migration is retained at
`/tmp/tango-bn5-package-parity/src/bin/model.rs`. Game behavior in the package
remains Luau; this temporary oracle is not a runtime dependency.
