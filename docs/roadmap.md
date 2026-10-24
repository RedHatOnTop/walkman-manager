# Roadmap

Ordered by dependency and risk. A phase closes when its gate is demonstrated on real
players, not when its code compiles.

## P0 Foundation — done 2026-10-15

Workspace (`uwm-core`, `uwm-cli`, Tauri 2 shell, React UI), the remote build loop, the design
system, Korean and English, dark and light themes.

Gate: `spectre-run.sh all` green; the release binary starts on the Fedora desktop.

## P1 Devices

Detection across USB, adb, Fastboot and boot ROM modes; live overview per player; the
annotated drawing; known players remembered.

Gate: both units appear with correct model, firmware, battery, storage and output state;
unplug and replug is reflected without a restart; a stream started on the player shows its
real rate and format.

Next in this phase: host-side mass-storage mount detection for the A55 in storage mode,
an udev rule installer for `no-permission`, a screenshot of the player screen on request
(`screencap` on Android only, never automatic).

## P2 Music library and sync — in progress

Built 2026-10-15: library scan with tags, DSD headers, covers and quality classes; album
grid with per-player presence; per-volume listing over adb; dry-run plans against each
player's supported formats; `adb push` transfer with readback; a music section on the
device page; `uwm scan | music | plan` on the command line.

Built 2026-10-16: each player's music as its own library (Android media store with covers,
Walkman OS by folders) beside the computer's in one page; copying albums from a player to
the computer with size checks. Verified on the ZX507: 2,733 albums / 6,197 tracks read,
covers shown, an album pulled (MD5 equal to the player's files) and sent back to internal
storage. Later the same day: album details looked up in MusicBrainz and the iTunes store
and written into the files, on the computer or in place on a player behind a checked
backup, keeping SensMe analysis and other frames; legacy CP949 tags decoded on write.
SensMe channels: the analysis Music Center left in files read on the computer and from a
player, classified the way the firmware does with centroids taken from the user's own
firmware file (since 2026-10-24 every value that decides a channel); a channel board, per-channel lists and marks on album tracks. Verified on
the ZX507 card (120 of 6,197 analysed). Not yet: Walkman OS reads on a real A55, MP4
`ID32` on a real file.

Built 2026-10-19: SensMe analysis made for tracks that have none, with Music Center for PC's
own engine (the user's `MMLib11.dll`) run under Wine by our `mmhost.exe`, written into MP3
and FLAC as bytes. Verified on the 120 ZX507 files (channels equal on 119, the miss a stale
block) and in the app on the computer's library. Not yet: the in-place run on a player.
Not yet: the mounted mass-storage route for the NW-A55 (not connected this
session), the Walkman OS folder library on a real A55, playlists, conversion, `.lrc`
handled as more than a sidecar copy.

- Library: watch chosen folders on the computer; tags through `lofty`; DSF/DFF headers
  parsed directly; cover art cache; hi-res and DSD classification; albums, artists,
  playlists.
- What is on the player: the same index built over adb (Android `/sdcard/Music` and the SD
  card, Walkman OS `MUSIC/` on the internal and SD volumes) or over a mounted volume.
- Sync plans: choose albums or playlists per player and volume; a dry run lists copies,
  deletes and conversions with the space they need; transfer by `adb push` per file with
  resume, or by copying to the mounted volume; M3U8 playlists with relative paths; lyrics
  (`.lrc`) and folder art alongside.
- Format rules per profile from the player's codec support, converting only what the
  player cannot play (ffmpeg, user-installed).

Gate: an album with hi-res FLAC, DSD and lyrics synced to each player appears in the
player's own music app and plays.

## P3 Firmware Studio

A builder for custom system images, for owners who bring their own firmware. Design and
slices: `docs/firmware-studio.md`.

- Vault: the owner drops in what they own (Sony UPG, the LineageOS "ford" ROM, a TrebleDroid
  GSI). The app downloads only files whose licence lets anyone pass them on (the GSI, Sony's
  GPL source); for the rest it opens `docs/guides/firmware-files.md`. Each file is
  identified by structure and hash and stored with its provenance. Nothing in the vault is
  ever uploaded or bundled. The unit's UPG key is read from the owner's own player.
- Kits, not recipes copied here: the NW-ZX507 chain is `walkman-zx507`'s build entry point
  (planned there for 2026-10-20), pinned by commit and driven by the app. The NW-A55 output
  is the Android container set, not a flash, once its chain is scripted in `nw-a55`.
- Options as a form: debloat sets, apps to embed, theme accent, wallpaper, boot animation,
  audio patches, root. Each option shows the step it changes.
- Output: the images, their hashes, a manifest of inputs and options, the full log.

Built 2026-10-20: the vault and identification (slice 1). The owner's files are recognised
by structure and hash against `kits/files.toml`, kept in place, and shown per kit; the
TrebleDroid GSI and Sony's GPL source download inside the app, and every other file opens
its section of the guide. Verified on
the 16 firmware and image files on this machine and in the app. Also built 2026-10-20:
building the ZX507 image through walkman-zx507's kit at a pinned commit, release or debug,
verified in the app on this machine. Later the same day: the kit downloaded as walkman-zx507's
published archive instead of a local checkout, its kernel downloaded from the release beside
its source, and Google's build-tools behind the Android SDK licence shown in the app;
verified from an empty data folder to an update installed and read back on the unit (devlog
that day). Not yet: the A55 chain. Only Sony's 4.06.00 for series `0000` is taken, by
decision (2026-10-21): other versions and series stay named in the vault, with no kit for
them.

Gate: from the same inputs, the Studio reproduces the hand-built ZX507 image (equal file
manifest: path, sha256, mode, owner, SELinux label, xattrs) and the A55 container set.
2026-10-21: a release build on a clean Fedora 44 VM and one on this computer give the same
manifest but for the two vdex files of dex2oat's `-j4` order (devlog that day).

## P4 Flash and rescue

- NW-ZX500 over Fastboot: flash the inactive slot, switch, boot, fall back; per-partition
  backups through adb before writing; a slot diagram that shows what is where.
- NW-A50 over MTKClient: a managed install (pinned commit, app-private Python venv, GPLv3,
  run as a separate process); guided BROM entry (player off, hold Volume Down + Play,
  connect) with live detection of 0e8d:0003; full eMMC and boot-partition backup; restore of
  single partitions from the user's own backup.
- Sony's own path: building and triggering UPG packages (`nwupg.py`, the
  `scsitool do_fw_upgrade` trigger).
- NXP serial download (`uuu`) for a ZX500 whose bootloader is gone: research first.

Every write: verified backup of the target, a dry run, a typed confirmation naming the
partition, post-write verification by readback.

Built 2026-10-20: installing a Studio build on an NW-ZX500, clean (unlock, slot b, wipe) or
update (slot b, data kept), with the drawn guide for every step the owner does by hand. The
update ran on the unit from the app, read back slot, kernel and files (devlog that day).
Built 2026-10-21: going back to stock, from the Studio's "Back to stock" through the kit's
`make-restore.sh` to the install page's restore mode (Sony's four images to slot b, `-w`,
the stock kernel and files read back). Both wiping modes ran on the unit from the app on
2026-10-21, back to stock and then a clean install of the release build, each read back
(devlog that day). Built 2026-10-22: a `fastboot` gate that tries the formatter `-w` would
run, and Google's platform-tools downloaded on request where it cannot (Debian, Ubuntu)
or where adb is missing; `-w` through the downloaded fastboot ran on the unit in the clean
install of 2026-10-23.

Built 2026-10-22: the NW-A50 kit's storage set onto the player's WALKMAN drive (lsblk and
udisks, each file under a temporary name, a read-back from a fresh mount), then Sony's own
update command (rockbox scsitool's `do_fw_upgrade` bytes) from a pkexec helper, the wait for
the drive to come back and the UPG's log read off it. Not yet run on the unit.

Gate: each read flow verified on both units; each write flow performed once with the
operator present and the result verified by readback and a boot.

## P5 Personalise and sound

- Wallpaper, boot animation and accent colour: live over adb where the system allows it,
  otherwise as Studio options.
- NW-A55 DAC panel: the CXD3778GF mixer controls already mapped (master switch, analog
  mute, output device, headphone amp, gains), read and set over `amixer`.
- NW-ZX507: the state of Sony's DSEE, EQ and DIRECT paths as the vendor HAL reports them.

Gate: a change made in the app is visible or audible on the player.

## P6 Product

Packaging (rpm, deb, AppImage), an updater, first-run onboarding, a no-marks build without
third-party logos, Windows and macOS (nusb, adb, fastboot and MTKClient all exist there),
and a final name. The packages speak both languages too: the desktop entry's `Comment` and
`GenericName` in Korean beside English (`bundle.linux.*.desktopTemplate`), and the
installers' own text where they have any.

Built 2026-10-21: the deb, rpm and AppImage from an Ubuntu 22.04 container
(`spectre-run.sh bundle`); the deb requires Ubuntu's udev rule and recommends adb, fastboot
and the Studio's tools, the rpm recommends the same, and the app downloads Google's
platform-tools where the computer's own fall short (2026-10-22). Tried on fresh Ubuntu 24.04 and Fedora 44 VMs
(`scripts/dev/vm.sh`), a whole build included. Not yet: the desktop entry in Korean, an
updater, Windows and macOS.

Before the repository goes public (it is private on GitHub until then):

- ~~A licence~~ (GPL-3.0-or-later since 2026-10-20), and the fonts' OFL notices kept beside it.
- History: a ZX507 Bluetooth address and network secret sat in a test fixture until
  2026-10-19 and are still in the commits before that, so this repository stays private and
  the public one starts from a fresh root (user decision 2026-10-23), as nw-a55 and
  walkman-a55 do: `RedHatOnTop/walkman-manager-private` keeps the history,
  `RedHatOnTop/walkman-manager` (private until it is flipped) gets `scripts/export-public.sh`'s
  commits through the `public` remote. The export leaves out `AGENTS.md`, writes home paths
  from `~`, and refuses a tree that names a test unit or this computer.
- ~~What the research notes publish~~: nothing (user decision 2026-10-24, reversing
  2026-10-19). `docs/sensme.md`, `docs/sensme-analyzer.md` and `scripts/re/` stay in this
  repository and the export leaves them out; how SensMe works is told on the user's site.
  Counts and structure may be public (48 clusters, 16 inputs, eight mood tests, the block's
  layout, how the engine is called and fed); the values that decide a channel may not.
- ~~The values that decide a channel~~: read from the owner's own firmware (2026-10-24, the
  user's call). `Table` finds the per-cluster tables by the STHF slot table between them and
  the clustering and mood-test functions by a hash of their masked code, then reads the
  weights from the clustering code's literal pool and the 68 thresholds from the mood tests'
  `cmp` and `sub` immediates. `library/sensme.rs` keeps the offsets and how the tests combine,
  no values. A table saved by an earlier build asks for the file again.
- The public repository's first two commits (4a7a0c8, 4783135) carry the notes and the
  tables: before the flip it starts again from a fresh root, which is a force-push (the
  user's word).
- ~~A README for owners~~: `public/README.md` replaces this one in the public repository
  (2026-10-23): what runs, on which players, what has not been run (the Unlock stage above
  all), how to build.
- The name and the Walkman, Sony and Hi-Res marks in the interface.
