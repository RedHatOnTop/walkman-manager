#!/usr/bin/env python3
"""Drives the real app on a private X server and screenshots it.

    xcap.py OUTDIR STEP ...

The app must already run with DISPLAY=:91 (start Xvfb :91 -screen 0 1440x940x24 and
the binary with GDK_BACKEND=x11 WAYLAND_DISPLAY= WEBKIT_DISABLE_DMABUF_RENDERER=1);
UWM_XDISPLAY names another server.
Steps: `sleep:SECONDS`, `click:X,Y`, `key:KEYSYM` (or `ctrl+a`), `type:ASCII TEXT`, `scroll:X,Y,N`
(N > 0 scrolls down), `shot:NAME` (saved as OUTDIR/NAME.png).
"""
import os
import subprocess
import sys
import time
from pathlib import Path

from Xlib import X, XK, display
from Xlib.ext import xtest

DISPLAY = os.environ.get("UWM_XDISPLAY", ":91")


def click(d, x, y, button=1):
    d.screen().root.warp_pointer(x, y)
    d.sync()
    time.sleep(0.08)
    xtest.fake_input(d, X.ButtonPress, button)
    xtest.fake_input(d, X.ButtonRelease, button)
    d.sync()


MODIFIERS = {"ctrl": "Control_L", "shift": "Shift_L", "alt": "Alt_L"}
NAMED = {" ": "space", "-": "minus", ".": "period", "'": "apostrophe", "/": "slash", ",": "comma"}
SHIFTED = {"_": "minus", ":": "semicolon", "?": "slash", "!": "1"}


def key(d, name):
    """`a`, `Return`, or a chord such as `ctrl+a`."""
    *mods, last = name.split("+") if len(name) > 1 else [name]
    held = [d.keysym_to_keycode(XK.string_to_keysym(MODIFIERS[m])) for m in mods]
    if len(last) == 1 and (last.isupper() or last in SHIFTED):
        held.append(d.keysym_to_keycode(XK.string_to_keysym("Shift_L")))
    code = d.keysym_to_keycode(XK.string_to_keysym(NAMED.get(last, SHIFTED.get(last, last))))
    for h in held:
        xtest.fake_input(d, X.KeyPress, h)
    xtest.fake_input(d, X.KeyPress, code)
    xtest.fake_input(d, X.KeyRelease, code)
    for h in reversed(held):
        xtest.fake_input(d, X.KeyRelease, h)
    d.sync()


def main() -> None:
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    d = display.Display(DISPLAY)
    for step in sys.argv[2:]:
        kind, _, arg = step.partition(":")
        if kind == "sleep":
            time.sleep(float(arg))
        elif kind == "click":
            x, y = map(int, arg.split(","))
            click(d, x, y)
        elif kind == "scroll":
            x, y, n = map(int, arg.split(","))
            for _ in range(abs(n)):
                click(d, x, y, 5 if n > 0 else 4)
                time.sleep(0.03)
        elif kind == "key":
            key(d, arg)
        elif kind == "type":
            for ch in arg:
                key(d, ch)
        elif kind == "shot":
            subprocess.run(["import", "-display", DISPLAY, "-window", "root", str(out / f"{arg}.png")], check=True)
        else:
            raise SystemExit(f"unknown step {step}")
        time.sleep(0.25)


if __name__ == "__main__":
    main()
