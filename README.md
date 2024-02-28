> [!WARNING] 
> **Tango is no longer actively supported by the author.**
> 
> **There are no further updates expected to Tango.** If you wish to become the maintainer of Tango, please email <merry.bowl9875@fastmail.com>.

# Tango

Tango is rollback netplay for Mega Man Battle Network.

## Supported games

| Name                                                  | Gameplay support            | Save viewer support                                |
| ----------------------------------------------------- | --------------------------- | -------------------------------------------------- |
| Mega Man Battle Network 6: Cybeast Falzar (US)        | ✅ Works great!             | 🤷 Folder, NaviCust                                |
| Mega Man Battle Network 6: Cybeast Gregar (US)        | ✅ Works great!             | 🤷 Folder, NaviCust                                |
| Rockman EXE 6: Dennoujuu Falzer (JP)                  | ✅ Works great!             | 🤷 Folder, NaviCust, Patch Cards                   |
| Rockman EXE 6: Dennoujuu Glaga (JP)                   | ✅ Works great!             | 🤷 Folder, NaviCust, Patch Cards                   |
| Mega Man Battle Network 5: Team Protoman (US)         | ✅ Works great!             | 🤷 Folder, NaviCust, Patch Cards, Auto Battle Data |
| Mega Man Battle Network 5: Team Colonel (US)          | ✅ Works great!             | 🤷 Folder, NaviCust, Patch Cards, Auto Battle Data |
| Rockman EXE 5: Team of Blues (JP)                     | ✅ Works great!             | 🤷 Folder, NaviCust, Patch Cards, Auto Battle Data |
| Rockman EXE 5: Team of Colonel (JP)                   | ✅ Works great!             | 🤷 Folder, NaviCust, Patch Cards, Auto Battle Data |
| Rockman EXE 4.5: Real Operation (JP)                  | ✅ Works great!             | ✅ Navi, Folder                                    |
| Mega Man Battle Network 4: Blue Moon (US)             | ✅ Works great!             | 🤷 Folder, NaviCust, Patch Cards, Auto Battle Data |
| Mega Man Battle Network 4: Red Sun (US)               | ✅ Works great!             | 🤷 Folder, NaviCust, Patch Cards, Auto Battle Data |
| Rockman EXE 4: Tournament Blue Moon (Rev 1 only) (JP) | ✅ Works great!             | 🤷 Folder, NaviCust, Patch Cards, Auto Battle Data |
| Rockman EXE 4: Tournament Red Sun (Rev 1 only) (JP)   | ✅ Works great!             | 🤷 Folder, NaviCust, Patch Cards, Auto Battle Data |
| Megaman Battle Network 3: Blue (US)                   | ✅ Works great!             | 🤷 Folder, NaviCust                                |
| Megaman Battle Network 3: White (US)                  | ✅ Works great!             | 🤷 Folder, NaviCust                                |
| Battle Network Rockman EXE 3: Black (Rev 1 only) (JP) | ✅ Works great!             | 🤷 Folder, NaviCust                                |
| Battle Network Rockman EXE 3 (Rev 1 only) (JP)        | ✅ Works great!             | 🤷 Folder, NaviCust                                |
| Megaman Battle Network 2 (US)                         | 🤷 Works, with minor issues | 🤷 Folder                                          |
| Battle Network Rockman EXE 2 (AdColle only) (JP)      | 🤷 Works, with minor issues | 🤷 Folder                                          |
| Megaman Battle Network (US)                           | 🤷 Works, with minor issues | 🤷 Folder                                          |
| Battle Network Rockman EXE (JP)                       | 🤷 Works, with minor issues | 🤷 Folder                                          |

## Building

1.  Install Rust.

    ```sh
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
    ```

1.  Install the Rust target and toolchain for `x86_64-pc-windows-gnu`.

    ```sh
    rustup target add x86_64-pc-windows-gnu
    rustup toolchain install stable-x86_64-pc-windows-gnu
    ```

1.  Install mingw-w64.

    ```sh
    sudo apt-get install -y mingw-w64
    ```

1.  Ensure mingw-w64 is using the POSIX threading model.

    ```sh
    sudo update-alternatives --install /usr/bin/x86_64-w64-mingw32-gcc x86_64-w64-mingw32-gcc /usr/bin/x86_64-w64-mingw32-gcc-win32 60 &&
    sudo update-alternatives --install /usr/bin/x86_64-w64-mingw32-gcc x86_64-w64-mingw32-gcc /usr/bin/x86_64-w64-mingw32-gcc-posix 90 &&
    sudo update-alternatives --config x86_64-w64-mingw32-gcc &&
    sudo update-alternatives --install /usr/bin/x86_64-w64-mingw32-g++ x86_64-w64-mingw32-g++ /usr/bin/x86_64-w64-mingw32-g++-win32 60 &&
    sudo update-alternatives --install /usr/bin/x86_64-w64-mingw32-g++ x86_64-w64-mingw32-g++ /usr/bin/x86_64-w64-mingw32-g++-posix 90 &&
    sudo update-alternatives --config x86_64-w64-mingw32-g++
    ```

1.  Build it.

    ```sh
    cargo build --target x86_64-pc-windows-gnu --release --bin tango
    ```

### Server

The server is the remote HTTP server-based component that Tango connects to. It doesn't actually do very much, so you can run it on absolutely piddly hardware. All it does is provide signaling by sending WebRTC SDPs around.

If you already have Rust installed, you can build it like so:

```sh
cargo build --release --bin tango-server
```

## Language support

Tango is fully internationalized and supports language switching based on your computer's language settings.

The order of language support is as follows:

-   **English (en):** This is Tango's primary and fallback language. All Tango development is done in English.

-   **Japanese (ja):** This is Tango's secondary but fully supported language. All text in the UI, barring some extremely supplementary text (e.g. the About screen) is expected to be available in Japanese. If new UI text is added, a Japanese translation SHOULD also be provided. Tango releases MUST NOT contain missing Japanese text.

-   **All other languages:** These are Tango's tertiary languages. Support is provided on a best effort basis and translations are provided as available.
