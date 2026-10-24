#!/usr/bin/env python3
"""Screenshots of the browser preview (`npx vite --host 127.0.0.1 --port 1430` in app/).

    shot.py URL OUT.png [--full] [--dark] [--en] [STEP ...]

`--en` shoots the English interface (the browser's language picks it); Korean otherwise.

Steps run in order before the shot: `click:TEXT` clicks the first element with that
text, `css:SELECTOR` clicks a selector, `hover:SELECTOR` hovers, `wait:MS` waits,
`shot:NAME.png` takes an intermediate shot beside OUT.
"""
import sys
from pathlib import Path

from playwright.sync_api import sync_playwright

FULL = "html,body,#root,.shell{height:auto!important;overflow:visible!important}.main{overflow:visible!important}"


def main() -> None:
    args = sys.argv[1:]
    full = "--full" in args
    dark = "--dark" in args
    en = "--en" in args
    args = [a for a in args if a not in ("--full", "--dark", "--en")]
    url, out, steps = args[0], Path(args[1]), args[2:]
    with sync_playwright() as p:
        browser = p.chromium.launch()
        page = browser.new_page(
            viewport={"width": 1360, "height": 880},
            locale="en-US" if en else "ko-KR",
            device_scale_factor=1,
            color_scheme="dark" if dark else "light",
        )
        page.goto(url, wait_until="networkidle")
        page.wait_for_timeout(600)
        for step in steps:
            kind, _, arg = step.partition(":")
            if kind == "click":
                page.get_by_text(arg, exact=False).first.click()
            elif kind == "css":
                page.locator(arg).first.click()
            elif kind == "hover":
                page.locator(arg).first.hover()
            elif kind == "wait":
                page.wait_for_timeout(int(arg))
            elif kind == "shot":
                page.screenshot(path=str(out.parent / arg))
            else:
                raise SystemExit(f"unknown step {step}")
            page.wait_for_timeout(350)
        if full:
            page.add_style_tag(content=FULL)
            page.wait_for_timeout(300)
        page.screenshot(path=str(out), full_page=full)
        browser.close()


if __name__ == "__main__":
    main()
