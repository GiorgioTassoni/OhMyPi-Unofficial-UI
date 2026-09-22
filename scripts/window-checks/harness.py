#!/usr/bin/env python3
"""A WebKitGTK harness for the real frontend, with the host stubbed.

    python3 scripts/window-checks/harness.py scripts/window-checks/window.py

The frontend must be served (`bun run dev` in `frontend/`, on 127.0.0.1:5173), and
`stub.js` beside this file stands in for the Tauri IPC.

Why this exists: the app's own window cannot be driven while the machine is in use —
synthetic input goes to whatever holds focus, and focus moves on its own here. This runs
the same frontend in the same engine (WebKitGTK, the one Tauri embeds on Linux), with a
page I can read and a host stub injected at document-start. What it does **not** cover:
Tauri itself, and the compositor's drag-and-drop (the drop event is delivered the way the
Rust side delivers it).
"""

import importlib.util
from pathlib import Path
import json
import sys
import time

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")

from gi.repository import GLib, Gtk, WebKit2  # noqa: E402

URL = "http://127.0.0.1:5173"
STUB = Path(__file__).with_name("stub.js")


class Harness:
    def __init__(self, width=1400, height=900):
        window = Gtk.Window()
        window.set_title("omp-verify")
        window.set_default_size(width, height)
        view = WebKit2.WebView()
        window.add(view)
        window.show_all()

        self.window = window
        self.view = view
        self.loaded = False

        manager = view.get_user_content_manager()
        with open(STUB, encoding="utf-8") as handle:
            source = handle.read()
        manager.add_script(
            WebKit2.UserScript.new(
                source,
                WebKit2.UserContentInjectedFrames.TOP_FRAME,
                WebKit2.UserScriptInjectionTime.START,
                None,
                None,
            )
        )

        view.connect("load-changed", self._on_load)
        view.connect("load-failed", lambda _view, _event, uri, error: print(f"[harness] load failed {uri}: {error}", flush=True))
        view.connect("web-process-terminated", lambda *_args: print("[harness] the web process died", flush=True))
        view.load_uri(URL)

    def _on_load(self, _view, event):
        if event == WebKit2.LoadEvent.FINISHED:
            self.loaded = True

    def pump(self, seconds):
        """Let the main loop run: page timers, layout and promises all need it."""

        deadline = time.time() + seconds
        while time.time() < deadline:
            while Gtk.events_pending():
                Gtk.main_iteration_do(False)
            GLib.MainContext.default().iteration(False)
            time.sleep(0.005)

    def wait_loaded(self, timeout=30.0):
        deadline = time.time() + timeout
        while not self.loaded and time.time() < deadline:
            self.pump(0.2)

        if not self.loaded:
            raise SystemExit("the frontend did not load from vite")

    def js(self, source, timeout=30.0):
        """Evaluate `source` in the page and return its string value."""

        outcome = {}

        def finished(view, task):
            try:
                result = view.run_javascript_finish(task)
                outcome["value"] = result.get_js_value().to_string()
            except Exception as error:  # noqa: BLE001
                outcome["error"] = repr(error)

        self.view.run_javascript(source, None, finished)

        deadline = time.time() + timeout
        while "value" not in outcome and "error" not in outcome and time.time() < deadline:
            while Gtk.events_pending():
                Gtk.main_iteration_do(False)
            GLib.MainContext.default().iteration(False)
            time.sleep(0.005)

        if "error" in outcome:
            raise SystemExit(f"js failed: {outcome['error']}\n--- source ---\n{source}")
        if "value" not in outcome:
            raise SystemExit(f"js timed out after {timeout}s:\n{source}")

        return outcome["value"]

    def json(self, source):
        """Evaluate JS that yields `JSON.stringify(...)`, and decode it."""

        raw = self.js(source)
        try:
            return json.loads(raw)
        except json.JSONDecodeError:
            raise SystemExit(f"expected JSON from the page, got: {raw!r}")

    def check(self, source, timeout=30.0):
        """Run an async check that publishes its verdict on `window.__step`.

        WebKit will not hand back a promise, so a check reports by assigning a JSON
        string to a page global and this polls for it.
        """

        self.js("window.__step = null; true")
        # The trailing `; "started"` matters: WebKit refuses to convert a promise as the
        # script's completion value, and an async IIFE's value *is* one.
        self.js(source + '\n; "started"')

        deadline = time.time() + timeout
        while time.time() < deadline:
            raw = self.js("String(window.__step ?? '')")
            if raw not in ("", "undefined", "null"):
                try:
                    return json.loads(raw)
                except json.JSONDecodeError:
                    raise SystemExit(f"expected JSON from the page, got: {raw!r}")
            self.pump(0.15)

        raise SystemExit(f"check produced no result in {timeout}s:\n{source[:400]}")

    def reload(self):
        """Load the window again, stub and all.

        A driver's later checks should not inherit the screen an earlier one left behind: this
        group reloads, which resets the mock host's world as well as the page, so what follows
        is asserted against a known window rather than against a leftover.
        """

        # `loaded` is a latch, and it is still set from the first load: without clearing it,
        # `wait_loaded` returns immediately and whatever the caller injects next lands in the
        # document that is being torn down.
        self.loaded = False
        self.view.load_uri(URL)
        self.wait_loaded()

    def snapshot(self, path):
        """Write a PNG of the window.

        Spelled out here rather than using a screenshot tool because the window under test is
        this GTK webview and nothing else: a shot of the harness is a shot of the real frontend
        with the mock host injected, which is exactly what a visual pass needs to look at.
        """

        surface = []

        def captured(_view, task, _data):
            try:
                result = self.view.get_snapshot_finish(task)
                surface.append(result)
            except Exception as error:  # noqa: BLE001
                print(f"[harness] snapshot failed: {error}", flush=True)

        region = WebKit2.SnapshotRegion.VISIBLE
        options = WebKit2.SnapshotOptions.NONE
        self.view.get_snapshot(region, options, None, captured, None)

        deadline = time.time() + 15
        while not surface and time.time() < deadline:
            self.pump(0.05)

        if not surface:
            raise SystemExit(f"the snapshot of {path} never arrived")

        surface[0].write_to_png(path)
        print(f"[harness] wrote {path}", flush=True)

    def paste(self):
        """Paste through WebKit's own editing command.

        `Gtk.test_widget_send_key` does not reach the page's paste handling — measured:
        no `paste` event fired at all — so this uses the webview's editing command, which
        is WebKit's real paste path and reads the real system clipboard.
        """

        self.view.execute_editing_command("Paste")


def load_driver(path):
    spec = importlib.util.spec_from_file_location("wk_driver", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)

    return module


def main():
    if len(sys.argv) < 2:
        raise SystemExit("usage: wharness.py <driver.py>")

    driver = load_driver(sys.argv[1])
    harness = Harness()
    harness.wait_loaded()

    report = driver.run(harness)

    # A driver that returns its report is the whole point of `checked` wrapping each one: the
    # checks print as they go, but the summary is what says whether anything threw — and a
    # check that threw looks exactly like a check whose subject was missing.
    failing = [
        name
        for name, value in (report or {}).items()
        if isinstance(value, dict) and "error" in value
    ]
    print(f"[harness] {len(report or {})} checks, {len(failing)} threw: {failing}", flush=True)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
