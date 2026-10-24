# Architecture

## Shape

```
app/src (React)  ──invoke/events──▶  app/src-tauri (shell)  ──▶  crates/uwm-core
                                                                 ├─ usb        nusb, descriptors only
                                                                 ├─ adb        the adb client binary
                                                                 ├─ probe      one shell round trip + parsers
                                                                 ├─ profile    profiles/*.toml, compiled in
                                                                 ├─ registry   USB + adb → one entry per player
                                                                 ├─ library    scan, art, remote, plan, transfer
                                                                 ├─ studio     firmware files: identify, vault, fetch;
                                                                 │             kits: copy, host check, build
                                                                 └─ flash      install sets: fastboot, plan, read-back
crates/uwm-cli ─────────────────────────────────────────────────▶ crates/uwm-core
```

`uwm-core` holds every piece of device knowledge, so the CLI and the GUI behave the same
and the parsers are tested without a player. The shell only schedules work and moves data.
Pipelines that already exist as scripts in the porting projects (`nw-a55`,
`walkman-zx507`) are driven as external steps rather than rewritten; see the roadmap's
Firmware Studio phase.

## Detection

`registry::Registry::scan` merges three sources:

1. **USB** (`usb::list`). Every device from Sony (054c), MediaTek (0e8d) and NXP (1fc9),
   plus anything exposing an ADB or Fastboot interface. Functions come from interface
   class triples (ADB ff/42/01, Fastboot ff/42/03, mass storage 08, MTP 06/01/01) and from
   fixed ids for boot ROMs (MediaTek 0e8d:0003 BROM, 0e8d:2000-family preloader, NXP
   i.MX 8M serial download). Nothing is opened.
2. **adb** (`adb devices -l`). The `usb:` field ties each adb serial to a USB port.
   Network adb targets are attached to the player whose `ro.serialno` they report.
3. **Known players** (`known-devices.json` in the app data directory). A player seen once
   keeps its model when it later shows up in Fastboot, where nothing can be asked.

A player gets a profile from its properties (`profiles/*.toml` `[[identify]]` rules), else
from its USB id. Unknown adb devices are identified once with five `getprop` calls; a
device that matches no profile is remembered as foreign and never probed again. A phone
that happens to be plugged in does not appear.

Modes: `running` (adb answers), `unauthorized`, `offline`, `storage-only`, `bootloader`
(fastboot interface), `download` (boot ROM or preloader), `no-permission`.

## Probe

`probe.sh` is one POSIX sh script, sent as a single `adb shell` argument. It prints
sections (`@@props`, `@@version`, `@@meminfo`, `@@power`, `@@mounts`, `@@df`,
`@@asound`) that `probe::parse` turns into a `Snapshot`:

- system: platform (Walkman OS or Android), firmware (from `ro.sony.swid`), build, kernel,
  slot, bootloader state, an Android container mounted under Walkman OS;
- power: from `/sys/class/power_supply`, battery plus any online external supply;
- storage: user volumes only, from `df -k` (busybox on Walkman OS, toybox on Android)
  joined with `/proc/mounts` for the filesystem;
- audio: the ALSA cards and every playback substream's state and `hw_params`, which is
  what actually reaches the codec. AudioFlinger state is not evidence of playback.

The model inside a family (A55/A56/A57, ZX505/ZX507) is told apart by the rated capacity
that fits the internal user volume, and the UI says so.

## Library and sync

`library::scan` walks the chosen folders (hidden entries skipped), reads tags and stream
properties with `lofty`, and parses DSF and DSDIFF headers itself (`library::dsd`), handing
their ID3v2 block to lofty as a bare MPEG tag. Four worker threads read files; a track
whose size and mtime match the previous index is reused unread. Tracks group into albums
by album tag and album artist, by album tag and folder when there is no album artist, and
by folder when untagged; `CD1`/`Disc 2` folders join their parent. Embedded front covers,
else `cover.jpg`-style folder images, become 440 px JPEG thumbnails named by the source
hash (`library::art`), served to the page through the `art:` URI scheme.

Quality follows Sony's line: lossless above 44.1/48 kHz or 16 bit is Hi-Res, DSD is its
own class. What a player plays comes from its profile's `[playback]` table, the Help
Guide's "Supported formats" page: codec, upper sample rate, and the 4 GB file limit.

`library::remote` lists a volume's music with one `find … -exec stat -c '%s|%Y|%n' {} +`
(busybox on Walkman OS, toybox on Android). Music roots: `/sdcard/Music` and
`/storage/<id>/Music` on Android, `<mount>/MUSIC` on Walkman OS.

`library::plan` is the dry run. Destinations mirror the path below the library root, with
FAT/exFAT-illegal characters replaced. Same path and size: skip; same path, other size:
replace; absent: copy; not playable: listed, never sent. Nothing on the player is deleted.
256 MB stay free for the player's own database. `library::transfer` pushes file by file
with `adb push` (cancellable, a half-written new file is removed), then lists the volume
again and checks every pushed file's size. On Android it then sends
`MEDIA_SCANNER_SCAN_FILE` for the top folders it wrote.

A player's music reads as a library of its own (`library::player`). On Android that is the
media store, the database the player's music app shows: one `content query` over
`content://media/external/audio/media` with `is_music=1` (6,197 rows from the ZX507 in
about 3 s), retried without `album_artist`, `genre` and `bitrate` where Android 9 lacks
them. The store packs the disc into `track` (2003 is disc 2, track 3), writes `<unknown>`
for a missing artist, and keeps mis-decoded legacy tags, so every text value goes through
`repair_legacy`. It reports no sample rate or bit depth, only a bit rate, so a lossless
track above 1536 kbps (16 bit, 48 kHz, stereo) counts as Hi-Res. Covers are
`content read --uri content://media/external/audio/albumart/<album_id>` through
`adb exec-out`, fetched on demand when a tile scrolls into view, at most four at a time,
and cached under a key built from the device and album id; an album without art answers
with a Java exception and is not asked again. Reading covers makes Android's media
provider write its own thumbnail cache to `/sdcard/Music/.thumbnails` (a `.nomedia`
folder), as the player's music app does when it shows them. Walkman OS has no store, so
its albums are folders: album = folder, artist = the folder above, title = file name
without its track number.

`library::pull` copies a player's tracks to the computer: into the first library folder
(else the system music folder), keeping the path below the player's `Music/`. A file
already there at the same size is skipped, a different file of that name is kept and the
track skipped, and each copy lands as `<name>.uwm-part` and is renamed only after its size
matches. After a pull the destination joins the library folders and is scanned.

`library::lookup` searches MusicBrainz (one request a second, retried on 503) and the iTunes
Search API (US store; JP for kana) in parallel over `ureq`, ranks releases by its own
title/artist/length score, and turns a chosen release into a proposal: album fields that
differ, and per file a matched release track (title, length and number, above a bar) with
its own changes. Cover images come only from `coverartarchive.org` and `*.mzstatic.com`
and are shown through the art cache. `library::tags` edits MP3 and FLAC as bytes
(`library::tagbytes`): only the ID3 tag or the FLAC metadata blocks are rebuilt, every
frame or Vorbis field that is not edited is kept byte for byte (lofty drops TYER
`20080711`, the iTunes sort frames and fields such as `ALBUM ARTIST`), padding absorbs the
size change, and the audio is streamed across unchanged. Legacy-encoded text in the edited
and repaired fields is written back decoded. An MP3 with only ID3v1 gets a v2.3 tag that
carries the v1 fields as the same bytes, because Android reads v2 and never falls back to
v1. MP4, Ogg Vorbis, Opus and ID3 tags too odd to splice (unsynchronised, extended header,
footer) go through lofty, after which frames it dropped are put back. Either way the
result lands as `<name>.uwm-part`, is read back, and is refused if any frame or field of
the original is missing. `tags::write_smfmf` puts an SMFMF block into an MP3 (`GEOB`
application/SMFMF, description `USR_SMFMF`) or a FLAC (APPLICATION `SMFM`) the same way,
and checks that the copy reads back to the same block; `uwm sensme-copy` and `uwm tag`
expose both on the command line. `library::retag` runs the writes: on the computer through a copy that is read back
before it replaces the file; on a player after an md5-checked backup into
`<data>/backups/<model>/<time>/`, writing in place with `cat` from `/data/local/tmp` so the
media store keeps its row, then checking the checksum and restoring the backup on a
mismatch.

`library::sensme` decodes the SMFMF block Music Center for PC writes into analysed files
(ID3 `GEOB` application/smfmf, FLAC APPLICATION `SMFM`, MP4 `moov/meta/ID32`) into BPM,
chorus position and the 86 feature bytes, which the index keeps per track. Channels are
computed the way the player's firmware does it: nearest of 48
centroids for the time channel, eight predicates for the moods. Every value that decides a
channel is Sony's (the centroids, each cluster's time of day and fallback mood, the input
weights, the predicates' thresholds), so `sensme::Table` reads them from the user's own
`libMediaStoreService.so` (or the JSON copy made on import) and nothing is shipped: it finds
the tables by the STHF slot table between them, the two functions by a hash of their code
with immediates and branch targets masked, and reads the weights from a literal pool and the
thresholds from the `cmp`/`sub` immediates at known offsets. `read_player` reads a player's files in
batches of 200 over one adb shell: every header in one `head -q -c 10`, ID3 tags copied
whole, FLAC walked from the computer in 64 KB windows; a cancel returns what was read.

`library::analyse` makes that block for files Music Center never saw, with Music Center's
own engine: `AVLib/MMLib11.dll`, COM class MusicAnalysis2, which imports nothing of
Gracenote's. The engine is the user's (an installed Music Center, any Wine prefix's, or a
file they choose) and is never shipped. `mmhost.exe` (`crates/uwm-core/mmhost/mmhost.c`,
built with mingw on the burst runner and taken into uwm-core by `build.rs` from
`UWM_MMHOST_EXE`) loads it and runs jobs from stdin: natively on Windows, under Wine
elsewhere (PATH, then Bottles' and Lutris' runners), in a prefix of the app's own with
Mono, Gecko and menu entries turned off. ffmpeg decodes to 44.1 kHz 16-bit stereo and
the PCM goes to the engine in 4096-byte InputPCM calls, because one field of the block
depends on how the PCM is cut. MP3 is decoded without gapless trimming and gets one frame
(1152 samples) of silence in front when a Xing/Info/VBRI frame directly follows the tag,
the way Music Center's decoder plays it. `analyse::run` writes each block with `tags::write_smfmf`
through the same loops as the tag editor (`retag::rewrite_computer` / `rewrite_player`),
so player files are backed up, checksummed and written in place.

## Firmware Studio

`studio::identify` reads a file's first 4 KiB for its structure and then hashes the whole
file once (SHA-256 and MD5 together, about 450 MB/s here):

- Sony's NWWM package (magic, SHA-224 hex, the declared payload size, so a cut download
  shows as `Truncated`);
- a Walkman OS UPG (the first 16 bytes are the MD5 of the rest);
- update zips by what they carry (`payload.bin`, or `system.new.dat` with its transfer list),
  with the device from `META-INF/com/android/metadata` or the updater script, read from the
  zip's central directory without unpacking;
- ext4 (superblock size and label), Android sparse, boot and vbmeta images, xz, gzip.

The hash is then looked up in `kits/files.toml`, which pins every file the Studio knows
and says where it is published. A file with the size and kind of a known one but other
content is `Changed`.

`studio::vault` keeps the owner's files where they are: path, size, modification time,
the identification. A file whose size or time moved is `Changed` until it is looked at
again, and fills no kit input. `kits/<kit>.toml` lists each kit's inputs as known-file ids;
`Vault::kits` reports which vault entry fills each. A `vault.json` that does not parse is
moved aside to `vault.json.bad-<secs>` rather than overwritten. `studio::fetch` downloads a
known file with a direct `url` into the app's data (`studio/files/`) under a `.part` name,
hashing as it goes; it is renamed only when its size and sha256 are the pinned ones, then
identified like any other file. The body is read on a thread of its own so that a stalled
connection gives up after 60 s without data and the owner's cancel takes effect at once. Only files whose
licence lets anyone pass them on have a `url`; the rest have `guide`, a section of
`docs/guides/firmware-files.md` the app opens in the browser (`catalog::GUIDE`).

The app's Studio page runs one job at a time on a thread (`studio-progress`,
`studio-changed`, `studio-done` events); files come from the picker, from a drop on the
window (Tauri's drag-and-drop event), or from a fetch. `uwm studio identify|add|kits` does
the same from the command line and shares the app's vault. The app keeps the vault in memory
and writes all of it back, so files added with the CLI while the app is open are lost at the
app's next save.

Known limit: a ZIP entry whose size or offset is in a ZIP64 extra field (`0xFFFFFFFF` in the
central directory) is not followed, so a device name kept in such an entry of an OTA over
4 GB reads as unknown. No pinned file has one.

### Building

A kit with a `[source]` is built by the device repository's own entry point, not by code
here. `kits/nw-zx500-android13.toml` pins `walkman-zx507` at a commit and names
`scripts/make-install.sh` and its arguments (`input:<id>` for a vault file, `key`, `stock`).
Its `restore` names a second entry point, `scripts/make-restore.sh`, which takes only the UPG
and the key and makes a stock set: a build of variant `stock`, which downloads nothing and
so needs no licence agreement.

- `studio::kit` downloads the kit's published archive (`[source] archive`, pinned by size and
  sha256) or, for a kit without one, checks the repository folder the owner picked
  (`RepoState`) and copies the pinned commit with `git archive | tar -x`, never the working
  tree. Either way it lands in `studio/kits/<kit>-<commit12>/`, and `kit/COMMIT`
  (`export-subst`) must name the pinned commit. Downloaded tools are shared across copies
  through a `tools` link to `studio/tools/<kit>`, a symlink: building runs on Linux only
  (the kits are bash with e2fsprogs), Windows through WSL is P6.
- `studio::licence` reads the text of the licence a kit's own downloads come under from the
  manifest its publisher keeps it in (`[licence]`; Google's `repository2-3.xml` for the
  Android SDK License) and keeps the owner's agreement in `studio/licences.json` by the
  text's sha256. A build waits for it and sets the kit's variable (`ANDROID_SDK_LICENSE`) to
  `accepted`.
- `studio::doctor` checks the `[host]` needs: tools on `PATH`, Java's release, the NDK and
  SDK folders, free space.
- `studio::build::run` starts `bash <entry> <args> <set-dir>` in its own process group
  (`process::stream`, two reader threads, SIGTERM then SIGKILL to the group on cancel) with
  `RELEASE`, `KEEP_SU`, `ADB_KEY`, `NDK`, `ANDROID_SDK` and the licence variable set. The kit prints `== total <n>`
  and `== <i> <step>`; everything else is log. Each build gets `studio/builds/<id>/` with
  `build.log`, `build.json` (`Record`: kit, commit, inputs with hashes, options, outcome,
  step times) and, when it succeeds, `set/` with the kit's `install.json` and images. A
  failed build's `set/` is removed; a build the app did not see end reads `Interrupted`.
- The UPG key is read from the owner's player over adb (`cat /vendor/usr/data/icx_nvp.cfg`,
  world-readable on stock and on the kit's Android); only the `_KAS_` line is kept, in
  `studio/keys/<profile>.cfg`, mode 0600.

### Installing

`flash::set` reads a set's `install.json` and rechecks every file's sha256 before anything
is sent. `flash::plan` turns the set and what the player says (`Facts`, over adb when it
runs, `fastboot getvar` when it is in the bootloader) into `Gate`s and `Stage`s:

- clean: model, Sony firmware line, OEM unlocking (when still locked), battery 50 %, no
  card; reboot to the bootloader, unlock, vbmeta_b, set slot b, boot_b, system_b, `-w`;
  the word to type is `userdata`.
- update: the kit's Android already runs on slot b (its marker file, unlocked, slot) and
  is the same variant (`ro.adb.secure`); battery 30 %; the three writes only; the word is
  `system_b`.
- restore, only for a stock set (`Mode::fits`): model, Sony firmware line, the bootloader
  known to be unlocked, battery 50 %, no card; vbmeta_b, boot_b, system_b and vendor_b as
  Sony made them (`InstallSet::validate` allows vendor and no flags for stock only), set
  slot b, `-w`; the word is `userdata`. The bootloader stays unlocked.
- all end with reboot, wait for the boot, wait for adb (after a clean install or going back
  the owner turns USB debugging back on, or skips the read-back), and `flash::verify`: slot
  suffix, `/proc/version` against the set's kernel banner, the set's files by sha256, and
  the files that must be absent (for stock, the kit's marker file).
- an update then runs `cmd package bg-dexopt-job`. The kit's builds share the GSI's
  fingerprint, so Android takes an update for no OTA and leaves the odex it compiled
  against the earlier system until its next idle charge. A failure there goes to the log
  and leaves the checked install done.

`flash::fastboot` runs fastboot through the same stream as the kit and reads its
`Sending 'x' (n/m)` and `OKAY` lines into the bar. The app's install job runs on a thread
and emits `install`; unlock, the writes and the wipe are not cancelled half way.
`Fastboot::unlock` gives `flashing unlock` and its read-back one budget (25 minutes): the
bootloader is silent while it erases, so a command that times out or loses the player on USB
is followed by `getvar unlocked` every 5 s until it reads yes, while a bootloader that
answered (OKAY, or a refusal) is read once. Its tests run it against a fastboot script that
plays each of those bootloaders.

## Shell

`app/src-tauri/src/devices.rs` runs two threads: a USB hotplug watcher that only wakes the
scanner, and the scanner itself, which rescans on wake (after a 400 ms settle and again
1.6 s later) and every 2.5 s, and emits `devices` when the list changes. Snapshots are
read on request in a blocking task outside the registry lock.

Commands: `environment`, `profiles`, `scan_devices`, `rescan_devices`, `known_devices`,
`device_snapshot`; for the library `library_state`, `library_add_root` (folder picker
through the dialog plugin), `library_remove_root`, `library_rescan`,
`library_cancel_scan`, `device_music`, `sync_plan`, `sync_start`, `sync_cancel`,
`player_library`, `pull_plan`, `pull_start` (`sync_cancel` stops either direction);
`lookup_search`, `lookup_cover`, `lookup_propose`, `retag_start`, `retag_forget`;
`sensme_import` (file picker, then every index reclassified), `sensme_read`. Events:
`devices`, `library-scan`, `library`, `sync-progress`, `sync-done`, `pull-progress`,
`pull-done`, `retag-progress`, `retag-done`, `sensme-progress`, `sensme-done`. Errors cross the boundary as an `ErrorReport` (below). The `art:` scheme answers
off the webview thread, since a player cover may take an adb round trip.

`library.rs` keeps the folder list in `library.json` (app data) and the index plus covers
in the app cache. A scan runs at start-up and after every folder change; plans are kept
by id until they are carried out, and one transfer runs at a time.

## Errors in two languages

The app speaks Korean and English natively, so no English sentence from the backend is
shown as the message. `uwm_core::ErrorReport` carries:

- `kind`: stable, for logic (`busy`, `no-such-plan`, `io`…);
- `say`: the `err.<say>` string the UI shows, filled with `vars` (`{tool}`, `{secs}`, `{n}`);
- `detail`: the value the sentence is about, shown under it as is: a path, an address, a
  tool's or the OS's own words;
- `message`: the English, for logs and the command line.

`Error::Unsupported` takes a `Why`, one code per refusal (`Why::NoWine`,
`Why::MusicNotRead`…), built with `Error::unsupported(why, message)` and optionally
`.with_detail()` / `.with_var()`; the other variants map to fixed says (`tool-missing`,
`timeout`, `io-not-found`, `io-no-space`…). A test fails when any code lacks its string in
the ko or en table. Per-file failures (`transfer::Failure`, lookup failures, the Studio's)
carry an `ErrorReport` too. The app shows them through `components/ErrorText.tsx`.

## Interface

React 19, no router or state library. Providers subscribe to backend events before their
first read, so a change announced in between is not lost. `state/data.tsx` owns the scan, the known players
and the profiles, and a readings provider polls every running player: the one on screen
every 4 s, the others every 15 s, only while the window is visible. Out of Tauri, in a Vite
dev server, `api/mock.ts` serves the recorded fixtures so the interface can be worked on
without a player.

## Build

See README. Releases are built inside a Fedora 44 container on an aws-burst runner, driven
from Spectre, so the desktop never compiles.
