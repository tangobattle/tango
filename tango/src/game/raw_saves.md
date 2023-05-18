# .raw saves

The bundled saves in Tango are not in .sav format: they are instead in a raw format, namely:

-   Start and end of the save are trimmed to region the game reads.
-   Checksum is set to 0.
-   The save is unmasked, and the mask is set to 0.

The raw format means it's easy to edit the saves without having to rebuild and remask the save if changes are required.
