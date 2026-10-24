#!/usr/bin/env python3
"""Shows a private X server in a window on the desktop and passes the pointer and keys to it.

    xmirror.py [DISPLAY]

For flows where the owner watches the app on Xvfb (see xcap.py) or acts in it: typing a
confirmation, pressing a button. Keys go through as X keysyms, so Hangul input does not.
"""
import sys

import gi

gi.require_version("Gdk", "4.0")
gi.require_version("Gtk", "4.0")
from gi.repository import Gdk, GLib, Gtk  # noqa: E402
from Xlib import X, display  # noqa: E402
from Xlib.ext import xtest  # noqa: E402

SOURCE = sys.argv[1] if len(sys.argv) > 1 else ":91"
EVERY_MS = 250

src = display.Display(SOURCE)
root = src.screen().root
geometry = root.get_geometry()
W, H = geometry.width, geometry.height


def frame() -> Gdk.Texture:
    data = root.get_image(0, 0, W, H, X.ZPixmap, 0xFFFFFFFF).data
    # python-xlib hands the pixels back as bytes or, some replies, as a latin-1 str.
    if isinstance(data, str):
        data = data.encode("latin-1")
    return Gdk.MemoryTexture.new(W, H, Gdk.MemoryFormat.B8G8R8X8, GLib.Bytes.new(data), W * 4)


def to_source(picture: Gtk.Picture, x: float, y: float) -> tuple[int, int] | None:
    """The picture is scaled to fit with its aspect kept, so the margins fall outside."""
    w, h = picture.get_width(), picture.get_height()
    scale = min(w / W, h / H)
    sx, sy = (x - (w - W * scale) / 2) / scale, (y - (h - H * scale) / 2) / scale
    if 0 <= sx < W and 0 <= sy < H:
        return int(sx), int(sy)
    return None


def pointer(picture: Gtk.Picture, x: float, y: float) -> bool:
    at = to_source(picture, x, y)
    if at:
        root.warp_pointer(*at)
        src.sync()
    return at is not None


def press(kind: int, detail: int) -> None:
    xtest.fake_input(src, kind, detail)
    src.sync()


def activate(app: Gtk.Application) -> None:
    window = Gtk.ApplicationWindow(application=app, title=f"Walkman Manager on {SOURCE}")
    window.set_default_size(int(W * 0.75), int(H * 0.75))
    picture = Gtk.Picture(content_fit=Gtk.ContentFit.CONTAIN, can_shrink=True, focusable=True)
    picture.set_paintable(frame())
    window.set_child(picture)

    motion = Gtk.EventControllerMotion()
    motion.connect("motion", lambda _c, x, y: pointer(picture, x, y))
    picture.add_controller(motion)

    click = Gtk.GestureClick(button=0)

    def pressed(gesture: Gtk.GestureClick, _n: int, x: float, y: float) -> None:
        picture.grab_focus()
        if pointer(picture, x, y):
            press(X.ButtonPress, gesture.get_current_button())

    click.connect("pressed", pressed)
    click.connect("released", lambda g, _n, x, y: press(X.ButtonRelease, g.get_current_button()))
    picture.add_controller(click)

    scroll = Gtk.EventControllerScroll(flags=Gtk.EventControllerScrollFlags.VERTICAL | Gtk.EventControllerScrollFlags.DISCRETE)

    def scrolled(_c: Gtk.EventControllerScroll, _dx: float, dy: float) -> bool:
        button = 5 if dy > 0 else 4
        for _ in range(max(1, round(abs(dy)))):
            press(X.ButtonPress, button)
            press(X.ButtonRelease, button)
        return True

    scroll.connect("scroll", scrolled)
    picture.add_controller(scroll)

    keys = Gtk.EventControllerKey()

    def key(kind: int, keyval: int) -> bool:
        code = src.keysym_to_keycode(keyval)
        if code:
            press(kind, code)
        return True

    keys.connect("key-pressed", lambda _c, keyval, _code, _state: key(X.KeyPress, keyval))
    keys.connect("key-released", lambda _c, keyval, _code, _state: key(X.KeyRelease, keyval))
    picture.add_controller(keys)

    def refresh() -> bool:
        picture.set_paintable(frame())
        return True

    GLib.timeout_add(EVERY_MS, refresh)
    window.present()


if __name__ == "__main__":
    mirror = Gtk.Application(application_id="dev.personnya.walkman-manager.xmirror")
    mirror.connect("activate", activate)
    sys.exit(mirror.run([]))
