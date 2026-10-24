# Firmware Studio

Roadmap P3. The owner brings the firmware and ROM files, and the app turns them into images
ready to install, with our own patches applied. What we ship is the kit: our patches, the
scripts, pinned download references and this app. Images are never shipped. Sony's apps,
libraries, boot images and boot animation, the LineageOS and TrebleDroid images, and every
image built from them stay on the owner's machine.

## What exists today (2026-10-19)

Both players were built by hand, in their own repositories.

### NW-ZX507 (`walkman-zx507/`)

- **Inputs:**
  - TrebleDroid `system-td-arm64-ab-vanilla.img.xz` `ci-20230905`, sha256 `e3bfcc02…`.
  - Sony's 4.06.00 `.UPG`, from Sony's own update server (`info.update.sony.net/PA001`).
  - Sony's GPL kernel tarball.
  - microG, Aurora and Firefox, pinned by sha256 and signing certificate.
- **UPG:**
  - The 128-byte header has the magic `NWWM`, the SHA-224 of the payload and the payload size.
  - The payload is AES-256-CBC with no padding. Its key and IV are the unit's own `_KAS_` field
    in `/vendor/usr/data/icx_nvp.cfg`, which is readable over adb on stock and on our A13 alike.
  - Inside is an A/B OTA zip whose `payload.bin` uses full operations only: REPLACE, REPLACE_BZ,
    REPLACE_XZ and ZERO.
  - Tools: `decrypt-upg.sh`, `extract-partition.py`.
- **Image:**
  - About 17 steps, from `make-debug-gsi` to `drop-trebleapp`, each a script that edits the
    ext4 image file with `debugfs`. There is no loop mount and no root.
  - `chain-gsi.sh` runs a list of steps.
  - Patches to Sony and GSI binaries are gated on the input's sha256.
- **Boot:**
  - Stock `boot.img` with our kernel (`kernel/patches`, `kernel/build.sh`) swapped in by
    `repack-boot.py`.
  - The kernel build takes about 1000 s on a c7i-flex.large.
- **Install:**
  - `fastboot flashing unlock` (wipes the unit, about 10 min silent), then `vbmeta_b` (stock,
    verity off), `boot_b` and `system_b`.
- **Gaps:** that repository's plan for 2026-10-20 covers all of these:
  - There is no single build entry point.
  - There is no release variant: the debug `/init` wrapper, the permissive kernel domain and the
    host's adb key are still in the image.
  - The odex for the patched `services.jar` and SystemUI is compiled on a booted unit, so a fresh
    build needs two flashes.
  - Operator paths are hardcoded: the adb key, the serial, the NDK and the SDK.

### NW-A55 (`nw-a55/`)

- **Android is not flashed.** It runs as a container on the stock Walkman Linux, pushed over adb
  by `tools/android-boot.sh` (kernel modules, `container.tgz`, `system.img`), and is gone after a
  reboot.
- **Inputs:**
  - The LineageOS 12.1 "ford" zip (`sdat2img`).
  - Amazon's Fire OS 5.3.6.4 update, only for the container's `init`, `healthd` and ueventd
    rules from its boot ramdisk; the Fire OS userland itself was dropped. The LineageOS ford
    `boot.img` ramdisk carries the same four files (`init` and `healthd` static ARM, checked
    2026-10-20), so the update can go once `nw-a55` boots the container with them.
  - Sony assets from the owner's own unit (WalkmanUI's QML, images and fonts; the boot animation).
  - Sony's GPL kernel tree, for the modules.
- **Customisation:** `cm_customize.py` (through a sudo loop mount), `cm_debloat_debugfs.py`, the
  audio HAL, the wallpaper inside `framework-res.apk`, the boot animation, WalkmanUI (Qt 5.15 for
  Android) and the A2DP-only `Bluetooth.apk`.
- **Gaps:** steps 3–7 were manual `debugfs` sessions recorded only in devlogs. The Qt toolchain
  is the heaviest dependency. (Scripted since as nw-a55's `kit/`, with WalkmanUI, the audio
  HAL and the modules from a pinned prebuilt release; slice 4.)

## Shape

```
uwm-core::studio
  vault     identify owner files by structure and hash, keep provenance
  upg       Sony NWWM: decrypt with the unit's key, read payload.bin, write partitions
  kit       a device's build kit: inputs, options, steps, outputs (data, versioned)
  runner    run a kit in a work dir, stream progress, cancel, write the result manifest
  doctor    host tools a kit needs, with versions
app: Studio page per player model; P4 (Flash and rescue) installs what the Studio made
```

- **Vault.**
  - Files the owner adds are recognised by structure: the UPG header and model, OTA zip, ext4
    superblock, `.img.xz`, `boot.img` magic, the ford zip layout, the Fire OS update.
  - They are then matched against the pins each kit declares.
  - A file that matches no pin is still described ("Sony NW-ZX500 UPG 4.08.00, series 0003"),
    but no recipe accepts it.
  - The vault keeps hardlinks or reflinks under the app's data with a provenance record: where
    it came from, when, the sha256, and what it was derived from.
  - Nothing in it is uploaded, and nothing in it is in the repository.
- **Where the files come from.**
  - The app downloads a file itself only when its licence lets anyone pass it on: the
    TrebleDroid GSI, Sony's GPL source and walkman-zx507's published kernel. It fetches them
    on the owner's request and checks them against the pins.
  - Sony's UPGs, the LineageOS ford zip and the Fire OS update are the owner's to download.
    The app's "받는 방법" opens `docs/guides/firmware-files.md` at that file's section, which
    says where the publisher put it; the publishers' addresses live in the guide, not in the
    app.
  - The UPG key comes from the owner's own unit, read once over adb in a flow they start. It
    is stored with the vault, never in a kit, the guide or the app.
- **Kits come from the device repositories.**
  - `walkman-zx507/scripts/build-image.sh` (their plan, step 1) is the ZX507 recipe.
  - The Studio pins a copy of the kit, with the commit it came from, and drives it. It does not
    reimplement the steps. A kit with a published archive (`git archive` of the commit, as a
    release asset, pinned by sha256) is downloaded on its first build; otherwise the owner
    names a checkout that holds the commit.
  - What a kit itself downloads under someone else's licence waits for the owner's agreement:
    the kit names the licence and the manifest its publisher keeps the text in, the app shows
    that text and passes the agreement in an environment variable.
  - A kit needs:
    - one entry point that takes inputs as arguments;
    - options as flags;
    - no operator paths: adb key, serial, NDK, SDK and the cache directory all come in from
      outside;
    - one progress line per step on stdout;
    - exit status not zero on failure;
    - a manifest of its outputs;
    - pinned downloads, verified by sha256;
    - a release switch.
- **Runner.**
  - Runs in a work directory under the app's cache, reflinked where the filesystem allows.
  - Linux first, since the kits use `debugfs`, `xz`, `zip`, a JDK and NDK clang. Windows through
    WSL is P6.
  - The result manifest holds: kit commit, input hashes, options, output hashes, the full log.
- **Gate (from the roadmap):** from the same inputs, the Studio reproduces the hand-built image.
  Byte equality is not possible, because debugfs allocation order differs between runs. The bar
  is the file manifest the ZX507 plan defines: path, sha256, mode, owner, SELinux label, xattrs.

## Slices

1. **Vault and identification.** Depends on nothing. Built 2026-10-20 (devlog that day).
   - Recognise every input on this machine:
     - the eight UPGs in `walkman-zx507/firmware`;
     - the TrebleDroid xz and img;
     - the stock images in `SHA256SUMS`;
     - the ford zip;
     - the Fire OS update.
   - Reject a truncated or renamed-wrong file.
   - Studio page lists them per player.
2. **UPG unpack in Rust.**
   - Decrypt with the key read from the connected unit, check the SHA-224, extract `system`,
     `vendor`, `boot` and `vbmeta` from `payload.bin`.
   - Gate: hashes equal `SHA256SUMS`, and the key never leaves the vault.
3. **ZX507 build.** Built 2026-10-20 as a run of walkman-zx507's `scripts/make-install.sh`
   at the pinned commit (devlog that day): release or debug, keep root; the other options
   below are the kit's defaults for now. Going back to stock (2026-10-21) is the same kind of
   run of the kit's `scripts/make-restore.sh`: the four stock partitions from the UPG as an
   install set for slot b, which the install page writes in its restore mode.
   - Options form: microG, Firefox, theme accent, wallpaper, root yes/no.
   - Run, progress, result.
   - Gate: manifest diff against the hand-built image empty or explained.
4. **NW-A55 container bundle.** Built 2026-10-22 as a run of nw-a55's `kit/make-install.sh`
   at the pinned commit (devlog that day). Inputs: the owner's backup (`WMBACKUP`, kept in
   the vault by its `manifest.txt`, every image's md5 checked), the ford zip and the Fire OS
   update. The output is a storage set: what goes onto the player's WALKMAN drive under the
   paths it has there (the boot set with its MANIFEST, `fire-system.img`, an empty
   `android-data.img`) and the picker UPG built with the key the backup's recovery takes, all
   described by `install.json`. No variants: the kit takes Walkman One's BBDMP2 build only.
   - Install: copy in `install.json`'s order (an update keeps `android-data.img`), read back
     from a fresh mount, then Sony's update command through pkexec; the updater runs the UPG
     and the page reads its log off the drive. Built, not yet run on the unit.

## Answered by the owner (2026-10-20)

- Firmware (2026-10-21): only Sony's 4.06.00 for series `0000`, the build the patches are
  made against. Other versions and series are named in the vault but no kit takes them, and
  none is planned.

- Kernel: prebuilt, published as a walkman-zx507 release with its complete source beside it.
- Root: a release build drops `su`; "keep root" keeps it.
- Google's SDK tools: the app shows the Android SDK licence and downloads them on agreement.
  The NDK goes away once the kit's own libraries ship prebuilt. Built 2026-10-20: the kit's
  libraries and VolumeDial come from walkman-zx507's `prebuilt-1` release, and Google's
  build-tools 34 download only with `ANDROID_SDK_LICENSE=accepted`, which the app sets once
  the owner agreed to the text it read from Google's repository manifest.
