Unmodified Luau Analysis sources and supporting headers from Luau 0.736,
commit c2ec0d4e5ca50796ba174a7565298f59aa572268:
https://github.com/luau-lang/luau/tree/0.736

The package checker compiles Analysis here and links it with the same upstream
Ast, Compiler, Config, Common and VM libraries supplied by mlua's luau0-src.
Keep this snapshot and Cargo.lock's luau0-src version in sync when upgrading.
The runtime uses mlua; no additional VM or compiler is built here.

Files retain their upstream MIT license; see LICENSE.txt. Tango's resolver
bridge is in ../../src/checker/bridge.cpp.
