#!/usr/bin/env python3
"""The settings screen, photographed inside the window it actually runs in.

Not a check: a visual pass. The checks assert behaviour; this writes PNGs of the screen with
the mock host's world in place, which is the only way to look at spacing, the mono/sans split,
and whether a row reads the way `docs/reference/02-settings.webp` does.

Everything here is synchronous JS (a click) followed by `pump`, because the harness's `js`
returns a string: a promise would arrive as "Unsupported result type", which is how this file
learned to drive the page the way `window.py` does.

    python3 scripts/window-checks/harness.py scripts/window-checks/shots.py
"""

import importlib.util
from pathlib import Path

HERE = Path(__file__).parent
OUT = Path("/tmp/omp-settings-shots")

spec = importlib.util.spec_from_file_location("window_checks", HERE / "window.py")
window_checks = importlib.util.module_from_spec(spec)
spec.loader.exec_module(window_checks)


def click(harness, selector):
    harness.js(
        f'document.querySelector({selector!r})?.dispatchEvent(new MouseEvent("click", {{ bubbles: true }})); true'
    )
    harness.pump(1.0)


def run(harness):
    harness.js(window_checks.PRELUDE)
    harness.pump(0.5)
    OUT.mkdir(exist_ok=True)

    # The entry point a user takes, and the page the nav lands on.
    click(harness, "[data-action='settings']")
    harness.pump(0.6)
    harness.snapshot(str(OUT / "01-general.png"))

    # Every kind of row on one page: a toggle, a gated number, a record, a list.
    click(harness, "[data-view='settings'] [data-section='model-and-providers']")
    harness.snapshot(str(OUT / "02-model-and-providers.png"))

    # A credential row: presence, and no way to read it back.
    click(harness, "[data-view='settings'] [data-section='memory']")
    harness.snapshot(str(OUT / "03-memory-secret.png"))

    # The escape hatch, with the plan the host would return for the file as it stands.
    click(harness, "[data-view='settings'] [data-section='raw-config']")
    harness.pump(0.8)
    harness.snapshot(str(OUT / "04-raw-config.png"))

    return {"shots": sorted(path.name for path in OUT.glob("*.png"))}
