# Ultimate Walkman Manager

A desktop manager for Sony Walkman players on Linux: what the player is doing right now,
your music on the computer and on the player, and newer Android built from your own files
and installed from the app. It is unofficial and community-made: not made by, affiliated with
or endorsed by Sony.

**Beta: not a stable release.** Every flow below has run on one player of each model, the
author's, from a Fedora 44 desktop, and nowhere else. The Android it installs is not stable
either: the NW-A50 series' is alpha ([walkman-a55](https://github.com/RedHatOnTop/walkman-a55)),
the NW-ZX500's beta ([walkman-zx507](https://github.com/RedHatOnTop/walkman-zx507)). Expect
bugs and changes between versions, and read [What has not been run](#what-has-not-been-run)
before you install anything on a player. The app comes with no warranty (sections 15 and 16
of the [GPL](LICENSE)).

## Players

| Player | Android the app installs | Tested on |
|---|---|---|
| NW-ZX500 series | Android 13: TrebleDroid's GSI with Sony's audio path, W.music and a kernel built from Sony's GPL source, through [walkman-zx507](https://github.com/RedHatOnTop/walkman-zx507)'s kit. Needs Sony's firmware 4.06.00, series 0000, on the player. Installed to slot b over fastboot; Sony's own firmware stays on slot a. | NW-ZX507 |
| NW-A50 series | Android 5.1 beside Walkman OS, through [walkman-a55](https://github.com/RedHatOnTop/walkman-a55)'s kit, built from a backup of your own player. Nothing is flashed: the set is copied onto the player's drive and a boot picker is installed by Sony's own updater. | NW-A55 |

Both are detected over USB in every mode the app knows: Android with or without USB
debugging approved, fastboot, the MediaTek boot ROM and preloader, and NXP's serial
download mode.

## What it does

- **Overview.** The player as it is, drawn to scale with its keys and jacks where Sony puts
  them, and its live state: output path and rate, battery, storage, system, hardware,
  connection.
- **Music.** The computer's folders and each player's own music as one library of albums;
  sending albums to a player and copying them back. SensMe channels for new music are
  computed with the analysis engine of your own Sony Music Center for PC 2.7.3 under Wine.
- **Firmware Studio.** Drop in the firmware and ROM files you have; the Studio tells what
  each one is and whether a player's build has everything it needs. Files under an open
  licence (TrebleDroid's GSI, Sony's GPL source) can be downloaded in the app; for the rest it
  says where to get them ([`docs/guides/firmware-files.md`](docs/guides/firmware-files.md)).
  Builds run the kits of walkman-zx507 and walkman-a55 on your computer.
- **Install.** NW-ZX500: a clean install, an update that keeps your data, and going back to
  Sony's firmware, each read back from the player afterwards. NW-A50: the set onto the
  WALKMAN drive and the boot picker through Sony's updater, then checked.

## What has not been run

- **Unlocking the bootloader from the app.** A clean install on a locked NW-ZX500 starts by
  running `fastboot flashing unlock`. That stage has never run from the app: the author's
  ZX507 was unlocked by hand (the
  [walkman-zx507 devlog](https://github.com/RedHatOnTop/walkman-zx507) has the session), and
  locking it again to try the app's stage is not something anyone should do to a player.
  When it was done by hand there was no prompt on the player's screen and the player wiped
  itself silently for about nine and a half minutes. Unlocking erases everything on the
  player.
- **Other players.** One unit per model. The other NW-ZX500 and NW-A50 models, and other
  firmware versions, have not been tried; the NW-ZX500 build takes only 4.06.00, series
  0000, on purpose.
- **Other systems.** Linux only. Windows and macOS are planned, not started.
- **Updates and releases.** There is no updater and no release package yet; build it from
  source below.

## Build

You need Rust (rustup reads the version from `rust-toolchain.toml`), Node.js 22 and the
WebKitGTK 4.1 and GTK 3 development packages:

```sh
# Fedora
sudo dnf install webkit2gtk4.1-devel gtk3-devel libsoup3-devel javascriptcoregtk4.1-devel \
  librsvg2-devel openssl-devel gcc gcc-c++ make pkgconf-pkg-config nodejs npm
# Ubuntu 22.04 or newer, Debian
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
  libjavascriptcoregtk-4.1-dev librsvg2-dev libssl-dev build-essential pkg-config
```

Then:

```sh
cd app
npm ci
npx tauri build               # the deb, rpm and AppImage
npx tauri build --no-bundle   # or only the program
```

Both land under `target/release/` at the top of the checkout: the program as
`walkman-manager`, the packages in `bundle/`.

SensMe analysis needs a small Windows helper that runs Sony's engine. Without it the app
builds and works, it only cannot analyse music. To include it:

```sh
i686-w64-mingw32-gcc -O2 -Wall -Werror -municode -static -s -o mmhost.exe \
  crates/uwm-core/mmhost/mmhost.c -lole32 -loleaut32
export UWM_MMHOST_EXE="$PWD/mmhost.exe"   # before building
```

The app talks to the player through `adb` and `fastboot`. Where the computer has no adb, or
a fastboot that cannot format userdata (Debian's and Ubuntu's cannot), the app downloads
Google's platform-tools once you agree to the Android SDK licence they come under, pinned by
revision and sha256. On Debian and Ubuntu, `android-sdk-platform-tools-common` gives your
user the udev rule to reach the player; Fedora 44 needed nothing extra. The Studio's builds
use `e2fsprogs`, `python3`, a Java 17 runtime, `patchelf`, `binutils`, `curl`, `unzip`, `zip`, `xz`
and `openssl`.

Work on the interface without a player: `cd app && npm run dev`, then open
`http://127.0.0.1:1430/`. In a plain browser the app shows recorded players and an invented
music library; `?scenario=none|zx|brom|unauthorized`, `?device=<id>`, `?page=library` and
`?page=studio` pick a state.

## Layout

| Path | Contents |
|---|---|
| `crates/uwm-core` | Device profiles, USB enumeration, adb, the probe and its parsers, the music library, the Studio, flashing |
| `crates/uwm-cli` | `uwm devices`, `uwm snapshot <id>` over the same core |
| `app/src-tauri` | The Tauri 2 shell: USB hotplug, commands, events |
| `app/src` | The React interface, in Korean and English |
| `profiles/` | One file per player family: how it is told apart, its USB ids, its body |
| `kits/` | The firmware files the Studio knows, and each build kit's inputs and pins |
| `docs/` | Architecture, design, the Studio, guides |
| `devlog/` | What was done and tried, by date |

## Firmware and distribution

The app ships its own code only. Firmware, vendor files and anything made from them come
from your own player or from where their publisher put them, and stay on your computer.

## Licence

GPL-3.0-or-later ([`LICENSE`](LICENSE)). The bundled fonts keep their own licence, the SIL
Open Font License: Wanted Sans (`app/src/assets/fonts/WantedSans-OFL.txt`) and DM Mono
(through its Fontsource package).

Sony, Walkman and the Walkman logo are trademarks of Sony Group Corporation. This project is
not affiliated with or endorsed by Sony.
