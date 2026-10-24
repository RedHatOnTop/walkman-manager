# Design system

This app's own visual language. It does not borrow another project's palette or
components.

## Idea

The players are the only objects. Each connected Walkman is rendered as the physical
thing, to scale, turned a little toward the viewer so its side keys and top jacks show,
and its screen shows what the manager can tell about it: the clock while idle, a
now-playing screen with the stream's rate while sound plays, the USB debugging prompt
while it waits for approval. Everything around the player reads like Sony's own spec
sheets and Help Guide: plain type, hairline rows, nothing that glows.

Three rules follow.

1. One accent, for one thing. "Hotline orange", after the orange HOTLINE button on the
   first Walkman, marks sound that is playing now: the output row, the jack in use, the
   arrows of the signal path. Actions are ink (`--ink` on `--on-ink`), selection is ink,
   presence marks are ink. A charging lamp is the player's own amber, not the accent.
2. Readings are rows, not headlines. Output, battery, free space and system sit in a
   `dl.specs` of label | value rows with a hairline under each, at body size. The battery
   is drawn as the players draw it in their status bar (`components/Battery.tsx`) or as a
   20-cell meter; there are no ring gauges, KPI strips or big counters.
3. Flat panels. A tile is `--tile` with a 1 px edge and radius 6–10; no gradients, inner
   highlights, light from above, halos or status dots. Labels are sentence case at
   12.5–13 px, never letter-spaced mono capitals.

## Colour

Tokens live in `app/src/styles/tokens.css`. Dark is the default, light follows the system
or the setting. Both are neutral graphite with no blue cast.

| Token | Dark | Light | Use |
|---|---|---|---|
| `--bg-0` | `#0c0c0d` | `#f3f3f1` | window |
| `--bg-1` | `#121213` | `#fafaf8` | sidebar, tiles, dialogs |
| `--bg-3` | `#212123` | `#e8e8e5` | the on-state of a segmented control |
| `--text-1..4` | `#ececed` → `#48484d` | `#111113` → `#b6b6ba` | four steps of emphasis |
| `--signal` | `#f47b2c` | `#e0600f` | sound playing now |
| `--warn` | `#e8c04e` | `#a87b05` | low battery, unlocked bootloader, awaiting approval |
| `--danger` | `#f2585e` | `#d9363e` | download and bootloader modes |

Quality in the library is named, not coloured: the tabs are plain text with counts. The
device page's format split keeps a grey scale with DSD as orange hatching.

## Type

- Wanted Sans Variable (400–1000, OFL), bundled: all prose, Korean and Latin. Stylistic
  set 06 is on for its straighter forms. Weights stop at 650.
- DM Mono (OFL), bundled through Fontsource: technical tokens only (paths, PCM devices,
  build ids, codec labels), never with Hangul in the same run.
- Scale: 40–58 the player's name on its own page, 30 page titles (Library, Firmware
  Studio, Settings), 22 kit names, 15–18 tile and dialog titles, 13.5–14.5 body, 12–13
  labels and secondary text.

## The render

`app/src/components/render/DeviceRender.tsx`. CSS 3D: an SVG front face (a metal frame
lit from the upper left, its polished chamfer, the glass inset by the model's lip, one soft
reflection, screen, charge lamp) plus a right face with the side keys and a top face with
the jacks, rotated `rotateX(-7deg) rotateY(-26deg)`. The side is shaded as the rounded
frame it is, bright a third of the way in; the keys are low domes in the body's own metal.
No marks are drawn: the SONY wordmark and the Walkman logo are Sony's, on the body and on
the install guide's boot screens alike (a plain bar stands for a boot logo). Each end of the
body has its own corner radius and the glass follows it: the ZX500 is square at the top under
its jacks (0.5 mm) and broadly rounded at the bottom (7.8 mm), with 3.3 mm of frame around the
glass; the A50 is 4.6 mm at both ends. The corners are drawn as narrow strips turned to their
normals; the side, top and corner faces run 3 px on under the front, and the side and top run
on under the corners, so no seam shows the page through. Body sizes
and part positions come from
`components/drawing/specs.ts`, measured from Sony's published dimensions, the Help Guide's
"Parts and controls" figures and product photos: the ZX500's six round keys with tactile
dots on play and volume +, its round HOLD knob and gold-ringed jacks; the A50's long volume
rocker, three round keys and slide HOLD. Keys are engraved with their glyphs. All renders
share one px/mm scale, so the ZX507 is visibly larger than the A55. Finishes are in
`finishes.ts`. The flat (unturned) render is used at small sizes: sidebar, send dialog,
transfer.

The technical drawing (`components/drawing/DeviceDrawing.tsx`) is the "Drawing" view of
the device page, with callouts carrying the readings.

The install guide (`components/guide/GuideStage.tsx`) stages the same render. Where the owner
has to press keys, numbered labels stand right of the player in the keys' order down the side,
each with an arrow to its key (volume − and power in stock recovery, the volume rocker and
▶II in the A50 picker); the key to press now, its ring and its arrow light up in time with
the player's screen.

## Layout

Sidebar 256 px; content up to 1320 px with 44 px gutters.

- Home: the players stand side by side (one player stands left of its name), each with
  its family, name, state and three spec rows; the latest albums sit on a shelf below.
- Device: a hero (render left, family, name, state, spec rows, Send music + Refresh),
  then a 12-column grid of tiles: Output 7 + Power 5, Storage 6 + Music 6, System 12,
  Hardware 6 + Connection 6. Under 1100 px every tile takes the full width.
- Library: the source switch (this computer, each player), the title and a line of
  counts, the folders; a sticky toolbar with Albums/SensMe, quality tabs, search and sort;
  a 190 px+ cover grid with hover actions. Album details and Send open as centred dialogs.
- Firmware Studio: the title and lead with Add files, then one panel per kit (the player
  at the left, a bay per file, "1 of 3 needed files" as a line) and the vault below.

## Motion

140 ms for hover and selection, 320 ms for page and value changes, one easing curve plus a
spring for small confirmations. Nothing pulses or glows; continuous motion is kept for
work in progress (spinners, the transfer and install progress) and for the install
guide's hands-on demonstrations (a tap, a swipe, a cable going in). `prefers-reduced-motion`
stops it.
