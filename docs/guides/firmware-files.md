# Getting the firmware files

[한국어](firmware-files.ko.md)

Walkman Manager ships no firmware. The Firmware Studio builds from files you download
yourself: add them with **Add files** or drop them on the window. The app reads each file
where it is and checks it against the size and hash pinned in
[`kits/files.toml`](../../kits/files.toml), so a copy that was cut short, or is a different
build, is named as such and is not used.

The app downloads a file itself only when its licence lets anyone pass it on: the
TrebleDroid image, Sony's GPL source and walkman-zx507's kernel. Every other file below
belongs to its publisher, and this page says where the publisher put it.

## Sony firmware updates

The NW-ZX500 series kit (Android 13) takes Sony's 4.06.00 update for series `0000`, the
build its patches are made against. The same file is what going back to stock writes. The
other files are recognised and named in the app, but no kit takes them, and none is planned.

Sony keeps them on the update server its players download their updates from:

| Model | Series | Version | File | Bytes |
|---|---|---|---|---|
| NW-ZX500 | `0000` | 4.06.00 | [`NW-ZX500_0000_V4_06_00_NW_WM_FW.UPG`](https://info.update.sony.net/PA001/NW-ZX500Series_0000/contents/0015/NW-ZX500_0000_V4_06_00_NW_WM_FW.UPG) | 907,457,808 |
| NW-ZX500 | `0001` | 4.06.00 | [`NW-ZX500_0001_V4_06_00_NW_WM_FW.UPG`](https://info.update.sony.net/PA001/NW-ZX500Series_0001/contents/0016/NW-ZX500_0001_V4_06_00_NW_WM_FW.UPG) | 910,338,016 |
| NW-ZX500 | `0002` | 4.06.00 | [`NW-ZX500_0002_V4_06_00_NW_WM_FW.UPG`](https://info.update.sony.net/PA001/NW-ZX500Series_0002/contents/0015/NW-ZX500_0002_V4_06_00_NW_WM_FW.UPG) | 907,411,888 |
| NW-ZX500 | `0003` | 4.04.00 | [`NW-ZX500_0003_V4_04_00_NW_WM_FW.UPG`](https://info.update.sony.net/PA001/NW-ZX500Series_0003/contents/0013/NW-ZX500_0003_V4_04_00_NW_WM_FW.UPG) | 486,161,168 |
| NW-ZX500 | `0003` | 4.08.00 | [`NW-ZX500_0003_V4_08_00_NW_WM_FW.UPG`](https://info.update.sony.net/PA001/NW-ZX500Series_0003/contents/0014/NW-ZX500_0003_V4_08_00_NW_WM_FW.UPG) | 486,061,952 |
| NW-ZX500 | `0004` | 4.06.00 | [`NW-ZX500_0004_V4_06_00_NW_WM_FW.UPG`](https://info.update.sony.net/PA001/NW-ZX500Series_0004/contents/0015/NW-ZX500_0004_V4_06_00_NW_WM_FW.UPG) | 907,426,160 |
| NW-ZX500 | `0005` | 4.06.00 | [`NW-ZX500_0005_V4_06_00_NW_WM_FW.UPG`](https://info.update.sony.net/PA001/NW-ZX500Series_0005/contents/0015/NW-ZX500_0005_V4_06_00_NW_WM_FW.UPG) | 907,414,432 |
| NW-A100 | `0000` | 4.06.00 | [`NW-A100_0000_V4_06_00_NW_WM_FW.UPG`](https://info.update.sony.net/PA001/NW-A100Series_0000/contents/0015/NW-A100_0000_V4_06_00_NW_WM_FW.UPG) | 906,889,168 |

Series `0003` is the build for China. Sony republishes files in place now and then; if a download no longer matches, the app says so instead of using it.

The update is encrypted with a key that each player holds. Neither this repository nor the
app carries that key: before a build, the Studio reads it from your own player over USB
debugging and keeps it on your computer only.

## Sony GPL source

Only needed to build the kernel on your own machine; the app can download it for you.
Sony publishes it on its source code distribution site, on the
[NW-A105 page](https://oss.sony.net/Products/Linux/Audio/NW-A105_Ver20211130.html), which
covers the NW-ZX500 series too: `gpl_source.tgz`, 304,730,290 bytes, MD5
`4ea368efdb62d167483d89e6ec60e2c3`.

## A kernel for the NW-ZX500

The NW-ZX500 series kit runs its own build of Sony's 4.14 kernel, patched for Android 13.
walkman-zx507 publishes that kernel as a release, `Image-zx507-4.14.78-1`, with its complete
source beside it (Sony's tree with the patches applied, the `.config` and the build script),
as the GPL asks. The Studio downloads it like the GSI: **Download** on the kernel's card.

To build it yourself instead, the kit repository's `kernel/build.sh` makes the same kernel on
an x86_64 Linux machine from Sony's GPL source (above), with AOSP's gcc 4.9 toolchain, which
it downloads. It takes about twenty minutes and several gigabytes:

```
kernel/build.sh gpl_source.tgz kernel-out
```

Add `kernel-out/Image` to the Studio like any other file. The app knows either kernel by the
version string inside, `4.14.78-g69dd21e`: it must stay that of Sony's kernel, or the
player's own Wi-Fi, audio and storage modules refuse to load.

## LineageOS for the Fire 7

The NW-A50 series kit (Android 5.1, run as a container on the player's own system) is
built on an unofficial LineageOS 12.1 for Amazon's Fire 7 (5th generation, codename `ford`):
`lineage-12.1-20200505-UNOFFICIAL-ford.zip` on
[AndroidFileHost](https://androidfilehost.com/?fid=4349826312261799008). The app checks it
by its MD5.

## Fire OS for the Fire 7

The NW-A50 series kit also takes Amazon's Fire OS 5.3.6.4 update for the same tablet, for
the container's `init`, `healthd` and device rules in its boot image. Nothing else in it is
used.

The file is `update-kindle-Fire_7_LC_5th_Gen-37.6.2.6_user_626549020.bin`, 664,431,377
bytes. Amazon published it at
`https://kindle-fire-updates.s3.amazonaws.com/YNbeTpooFBaqHGwW9t4OkFAzHt/update-kindle-Fire_7_LC_5th_Gen-37.6.2.6_user_626549020.bin`,
which now answers 403; the Internet Archive's Wayback Machine holds a capture of that
address.

## A backup of your NW-A50

The NW-A50 series kit takes WalkmanUI's Sony screens, images and fonts, the boot animation and
the key the player's updater checks packages with from the player itself. They come out of a
backup of its partitions that the player writes onto its own drive: the `WMBACKUP` folder,
about 2.4 GB. The backup also holds what is the player's alone (its serial number, Wi-Fi and
Bluetooth addresses and calibration): keep it on your computer and do not pass it on.

For now the kit takes the Sony files of the firmware its patches are made against, Walkman
One's (Sony build BBDMP2 of 2019-07-31); a backup of Sony's own 1.02 is named and refused.

To make the backup:

1. In the kit repository, [walkman-a55](https://github.com/RedHatOnTop/walkman-a55),
   `backup-upg/build.sh` makes two packages: `NW_WM_FW.UPG` for a player running Walkman One,
   and `stock/NW_WM_FW.UPG` for one on Sony's own firmware. The player refuses a package for
   the other firmware, and nothing happens.
2. Copy the one for your player to the root of the WALKMAN drive and eject the drive in your
   file manager. Leave the cable in.
3. Start the player's update with Rockbox's `scsitool`: `scsitool -d /dev/sgN do_fw_upgrade`,
   where `/dev/sgN` is the WALKMAN drive's first unit.
4. The player restarts, writes `WMBACKUP` (a few minutes) and starts as usual.
5. Copy the whole `WMBACKUP` folder to your computer and add its `manifest.txt` to the Studio.
   The app reads every image in the folder against the MD5 the player took as it wrote it.

## TrebleDroid

The NW-ZX500 series kit's system image is TrebleDroid's `ci-20230905` arm64 A/B vanilla
build. The app downloads it from the
[GitHub release](https://github.com/TrebleDroid/treble_experimentations/releases/tag/ci-20230905);
a copy you already have, packed (`.img.xz`) or unpacked, works as well.
