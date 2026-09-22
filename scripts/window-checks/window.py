#!/usr/bin/env python3
"""The window, driven inside WebKitGTK against the real frontend.

Step 9 turned the app from one session in a window into a set of threads, one sidecar each.
The checks here are the ones that refactor can break silently: which session a click resumes,
which thread the composer speaks to, whether a second thread's view replaces rather than
stacks on the first, and whether the sidebar's dots and unread marks follow the host's roster
rather than the window's own guesses.

Everything is a user gesture — a click on a row, typing in the box, pressing Enter — and
every assertion is either the DOM or the call the window made to the host.
"""

PRELUDE = r"""
window.__helpers = {
  sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
  all: (selector) => Array.from(document.querySelectorAll(selector)),
  text: (element) => (element?.textContent ?? "").trim(),
  button: (label) => Array.from(document.querySelectorAll("button"))
      .find((b) => (b.textContent ?? "").trim() === label),
  click: (element) => element.dispatchEvent(new MouseEvent("click", { bubbles: true })),
  calls: (cmd) => window.__calls.filter((c) => c.cmd === cmd),
  aside: () => document.querySelectorAll("aside")[0],
  /** A thread row, by the title it carries in its tooltip. */
  row: (title) => Array.from(document.querySelectorAll("aside button"))
      .find((b) => (b.getAttribute("title") ?? "").startsWith(title)),
  /** The dot is the row's first child: hollow for no sidecar, and the live states after. */
  dot: (row) => (row?.children[0]?.className ?? ""),
  /** The column on screen: the visible `main > section`, not every mounted thread. */
  visible: () => Array.from(document.querySelectorAll("main > section"))
      .find((s) => getComputedStyle(s).display !== "none"),
  box: () => document.querySelector("textarea"),
  /**
   * The right panel, found by what it *is* rather than by an attribute: the column is shared
   * with the diagnostics readout, and the one thing only this panel has is its two tabs.
   */
  threadPanel: () =>
    Array.from(document.querySelectorAll("aside")).find((node) =>
      Array.from(node.querySelectorAll("button")).some((b) =>
        ["Todos", "Files"].includes((b.textContent ?? "").trim().split(/\s+/)[0]),
      ),
    ) ?? null,
  threadPanelButton: (label) =>
    Array.from(document.querySelectorAll("aside button")).find((b) =>
      (b.textContent ?? "").trim().startsWith(label),
    ),
  /** A tree row, whose label carries its fold mark: `▸ workspace`, `▾ src`. */
  treeRow: (name) =>
    Array.from(document.querySelectorAll("aside button")).find((b) =>
      (b.textContent ?? "").trim().replace(/^[▸▾]\s*/, "") === name,
    ),
  /**
   * Make sure the right column is showing the *panel*.
   *
   * The diagnostics readout borrows the same column (`docs/12` §1), and the check that opens
   * it leaves it open — so a panel check has to claim the column rather than assume it.
   */
  /** The diagnostics readout, which borrows the same column, found by its own readouts. */
  diagnostics: () =>
    Array.from(document.querySelectorAll("aside")).find((node) =>
      (node.textContent ?? "").includes("negotiated v2"),
    ) ?? null,
  /**
   * A window control, by the action it performs.
   *
   * The titlebar is icon-only (`docs/12` §1), so a label is not a selector any more: these
   * buttons are found by `data-action`, which is stable across the icons they happen to draw.
   */
  action: (name) => document.querySelector(`header [data-action='${name}']`),
  /**
   * Put the panel in the column.
   *
   * Both toggles are real state and a blind click turns one *on* as easily as off — which is
   * how an earlier version of this helper hid the very panel it was sent to show. So: read the
   * column, change what is actually wrong, and wait for the result instead of assuming it.
   */
  async showPanel() {
    const button = (name) => document.querySelector(`header [data-action='${name}']`);

    // Re-read the column after *every* click rather than assuming what one did: the two
    // toggles are independent state, and a helper that guesses ends up turning the panel off
    // as often as on.
    for (let click = 0; click < 4; click += 1) {
      if (this.threadPanel() !== null) return;
      const which = this.diagnostics() !== null ? button("diagnostics") : button("panel");
      which?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      await this.waitFor(() => this.threadPanel() !== null, 1200);
    }
  },
  /** Bring a thread on screen and let its view settle. */
  async show(title) {
    this.row(title)?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await this.sleep(700);
  },
  /** A row's text, without the surrounding chrome. */
  panelText: () => (window.__helpers.threadPanel()?.textContent ?? "").replace(/\s+/g, " "),
  /** Open a row's context menu the way a user does: a right-click on the row. */
  async menu(title) {
    const row = this.row(title);
    row.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 120, clientY: 200 }));
    await this.sleep(200);
    return document.querySelector("[role='menu']");
  },
  menuItem: (label) => Array.from(document.querySelectorAll("[role='menu'] button"))
      .find((b) => (b.textContent ?? "").trim().startsWith(label)),
  searchPanel: () => document.querySelector("[aria-label='search every thread']"),
  async openSearch() {
    if (this.searchPanel()) return;
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true, bubbles: true }));
    await this.sleep(300);
  },
  async searchFor(text) {
    await this.openSearch();
    const input = this.searchPanel()?.querySelector("input");
    input.value = text;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await this.sleep(500);
  },
  panel: () => document.querySelector("[role='dialog'][aria-label='commands']"),
  // ------------------------------------------------------------- settings (`docs/12` §12)
  //
  // The screen is found by `data-view`, its rows by `data-row-key`, and a control by what it
  // *does*: these selectors are the interface, and a restyle must not be able to break them.
  settings: () => document.querySelector("[data-view='settings']"),
  settingsNav: () => Array.from(document.querySelectorAll("[data-view='settings'] [data-section]")),
  navSection: (id) => document.querySelector(`[data-view='settings'] [data-section='${id}']`),
  settingRow: (key) => document.querySelector(`[data-view='settings'] [data-row-key='${key}']`),
  settingRows: () => Array.from(document.querySelectorAll("[data-view='settings'] [data-row-key]")),
  settingsSearch: () => document.querySelector("input[aria-label='search settings']"),
  /** Bring a section on screen, since a row only exists once its section is drawn. */
  async showSection(id) {
    this.navSection(id)?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await this.sleep(400);
  },
  /** Leave settings the way it was found: the checks after these need the thread rail back. */
  async closeSettings() {
    if (!this.settings()) return;
    document.querySelector("[data-action='settings-back']")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await this.waitFor(() => this.settings() === null, 2000);
    await this.sleep(300);
  },
  async openSettings() {
    if (this.settings()) return;
    document.querySelector("[data-action='settings']")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await this.waitFor(() => this.settings() !== null, 2000);
    await this.sleep(300);
  },
  /** The escape hatch, which is the section whose id is `raw-config`. */
  async showHatch() {
    await this.openSettings();
    this.navSection("raw-config")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await this.waitFor(() => this.hatchEditor() !== null || this.hatchError() !== null, 2000);
    await this.sleep(200);
  },
  hatchEditor: () => document.querySelector("[aria-label='the config file, by hand']"),
  hatchError: () => document.querySelector("[data-view='settings'] [data-hatch-action='retry']"),
  hatchAction: (name) => document.querySelector(`[data-view='settings'] [data-hatch-action='${name}']`),
  hatchConfirm: () => document.querySelector("[aria-label='type CONFIRM to apply this plan']"),
  /** Replace the editor's YAML the way typing would, then let the plan settle. */
  async hatchType(text) {
    const box = this.hatchEditor();
    box.value = text;
    box.dispatchEvent(new Event("input", { bubbles: true }));
    await this.sleep(900);
  },
  async clickAction(element) {
    element?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await this.sleep(900);
  },
  // The agents panel (`docs/12` §9). Its title carries the live count, so the selector
  // matches the prefix rather than the whole label.
  agentsPanel: () => document.querySelector("[role='dialog'][aria-label^='Active agents']"),
  agentsText: () => (document.querySelector("[role='dialog'][aria-label^='Active agents']")?.textContent ?? "").replace(/\s+/g, " "),
  agentsTab: (label) =>
    Array.from(document.querySelectorAll("[role='dialog'][aria-label^='Active agents'] button"))
      .find((b) => (b.textContent ?? "").trim() === label),
  agentRow: (label) =>
    Array.from(document.querySelectorAll("[role='dialog'][aria-label^='Active agents'] button"))
      .find((b) => (b.textContent ?? "").trim().startsWith(label)),
  agentsSidebarRow: () =>
    Array.from(document.querySelectorAll("aside button"))
      .find((b) => (b.textContent ?? "").trim().startsWith("Active agents")),
  agentsSidebarCount: () => {
    const row = Array.from(document.querySelectorAll("aside button"))
      .find((b) => (b.textContent ?? "").trim().startsWith("Active agents"));
    const parts = (row?.textContent ?? "").trim().split(/\s+/);
    return parts[parts.length - 1] ?? "";
  },
  async openAgents() {
    const row = Array.from(document.querySelectorAll("aside button"))
      .find((b) => (b.textContent ?? "").trim().startsWith("Active agents"));
    row?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await this.waitFor(() => this.agentsPanel(), 4000);
    // The panel's open reads the rosters the host has not pushed, so a check that looks at
    // rows is looking at an answer rather than at whatever was already on screen.
    await this.waitFor(() => window.__calls.some((c) => c.cmd === "agents"), 4000);
    await this.sleep(300);
  },
  /**
   * Put a named thread on screen, from a state the check can trust.
   *
   * Two things make a thread check fail for the wrong reason, and both are real: a modal left
   * open by an earlier check swallows the click, and a row whose thread never opened turns
   * every assertion below it into a vacuous truth. So this closes whatever is in the way,
   * clicks the row, and answers whether the host now lists that thread as live — which every
   * check below reports, so a broken setup looks like a broken setup rather than like a
   * product bug.
   */
  async claim(title, id) {
    for (let attempt = 0; attempt < 3; attempt += 1) {
      const dialog = document.querySelector("[role='dialog']");
      if (dialog === null) break;
      dialog.querySelector("button[title='close']")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      await this.sleep(250);
    }

    this.row(title)?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await this.sleep(700);
    return (window.__world.live ?? []).some((entry) => entry.id === id);
  },
  /**
   * A *sidebar* row found by the label it renders. The sidebar is the first aside; the right
   * panel is another and its rows can carry the same words, which is how a rename check ends
   * up staring at a file.
   */
  rowByText: (text) =>
    Array.from(window.__helpers.aside()?.querySelectorAll("button") ?? [])
      .find((b) => (b.textContent ?? "").includes(text)),

  // ------------------------ notifications and suspension (`docs/14` step 14)
  //
  // The window decides whether a person is interrupted; the host only reports what happened.
  // So these selectors are the *presentation* contract: a notice per event, one status line,
  // one widget block, and the thread's own label when its sidecar was released.
  toasts: () => Array.from(document.querySelectorAll("[data-toast]")),
  notices: () => document.querySelector("[data-notices]"),
  statusLine: () => document.querySelector("[data-status]"),
  widget: () => document.querySelector("[data-widget]"),
  /** Put the window in front of someone, or away from them. */
  focus: (focused) => window.__focus(focused),
  /**
   * What the window asked the platform to show.
   *
   * Recorded at the *web* surface rather than at the plugin's Rust command, because that is
   * where the plugin's JavaScript actually arrives: `sendNotification` is
   * `new window.Notification(title, options)` and touches the command only through
   * `isPermissionGranted`'s fallback. Counting the command would count nothing, and a check
   * built on it would pass whatever the window did.
   */
  banners: () => window.__world.notifications,
  composerValue: () => (window.__helpers.box()?.value ?? ""),
  async waitFor(probe, timeout = 8000) {
    const deadline = Date.now() + timeout;
    while (Date.now() < deadline) {
      const value = probe();
      if (value) return value;
      await this.sleep(50);
    }
    return null;
  },
  async type(text) {
    const box = this.box();
    box.focus();
    box.value = text;
    box.dispatchEvent(new Event("input", { bubbles: true }));
    await this.sleep(80);
  },
  async press(key) {
    const box = this.box();
    box.focus();
    box.setSelectionRange(box.value.length, box.value.length);
    box.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
    await this.sleep(120);
  },

  // ----- the terminal drawer (`docs/12` §11, decision D6) -----------------------------------
  //
  // Found by its own marker rather than by position: it sits below the columns, and a check
  // that guessed at a wrapper would happily pass on an empty section.
  terminalPanel: () => document.querySelector("[data-terminal-panel]"),
  terminalShown: () => {
    const panel = document.querySelector("[data-terminal-panel]");
    return panel !== null && getComputedStyle(panel).display !== "none";
  },
  /** The titlebar's terminal control (`docs/12` §11). */
  terminalToggle: () => document.querySelector("header [data-action='terminal']"),
  /** One tab, by its label — the directory the shell was started in. */
  terminalTab: (label) =>
    Array.from(document.querySelectorAll("[data-terminal-panel] button")).find((b) =>
      (b.textContent ?? "").trim().startsWith(label),
    ),
  terminalTabs: () =>
    Array.from(document.querySelectorAll("[data-terminal-panel] button"))
      .map((b) => (b.textContent ?? "").trim().replace(/\s+/g, " "))
      .filter((text) => text !== "" && text !== "+"),
  /**
   * What the terminal is showing.
   *
   * The *visible* screen only: a tab in the background keeps drawing, and reading every
   * `.xterm-rows` would let one tab's output answer for another's.
   */
  terminalScreen: () =>
    Array.from(document.querySelectorAll("[data-terminal-panel] .xterm-rows"))
      .filter((node) => node.offsetParent !== null)
      .map((node) => node.textContent ?? "")
      .join("")
      .replace(/\u00a0/g, " "),
  /** The hidden textarea xterm reads keys from, for the tab on screen. */
  terminalInput: () =>
    Array.from(document.querySelectorAll("[data-terminal-panel] .xterm-helper-textarea")).find(
      (node) => node.offsetParent !== null,
    ) ?? null,
  /** One batch of output, the way the host posts it. */
  async terminalOutput(id, text) {
    window.__fire("terminal-output", { id, data: btoa(text) });
    await this.sleep(250);
  },
  async typeTerminal(text) {
    const input = this.terminalInput();
    input.focus();
    for (const key of text) {
      // `keyCode` is not decoration: xterm evaluates a key event with the legacy code, and a
      // synthetic event carrying only `key` is a keystroke nothing hears (measured — the host
      // recorded no input at all, which is the bug this helper had).
      const code = key === "Enter" ? 13 : key === " " ? 32 : key.toUpperCase().charCodeAt(0);
      input.dispatchEvent(
        new KeyboardEvent("keydown", { key, keyCode: code, bubbles: true, cancelable: true }),
      );
      await this.sleep(60);
    }
  },
  /** The close affordance on the tab on screen, found by what it does rather than its glyph. */
  terminalClose: () =>
    document.querySelector("[data-terminal-panel] [title='close this terminal']"),
  /**
   * The id of the tab on screen.
   *
   * Read from the DOM rather than from the window's state, because which emulator is *shown* is
   * the thing under test: two tabs in the same directory carry the same label, so only the id
   * can tell them apart.
   */
  terminalActive: () =>
    Array.from(document.querySelectorAll("[data-terminal]"))
      .find((node) => node.offsetParent !== null)
      ?.getAttribute("data-terminal") ?? null,
  /** Make sure the drawer is on screen, whatever the toggles currently say. */
  async showTerminals() {
    for (let click = 0; click < 3; click += 1) {
      if (this.terminalShown()) return true;
      this.terminalToggle()?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      await this.sleep(300);
    }
    return this.terminalShown();
  },
};

true
"""


def checked(report, name, harness, source, timeout=40.0):
    try:
        report[name] = harness.check(source, timeout=timeout)
    except SystemExit as error:
        report[name] = {"error": str(error)[:400]}
    except Exception as error:  # noqa: BLE001
        report[name] = {"error": repr(error)[:400]}

    print(f"--- {name}: {report[name]}", flush=True)


def run(harness):
    report = {}
    harness.js(PRELUDE)

    # ------------------------------------------------------- the browsing state
    checked(report, "sidebar", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.waitFor(() => h.row("Wire the sidebar"));
  const aside = h.aside();
  const rows = Array.from(aside.querySelectorAll("button"))
    .filter((b) => (b.getAttribute("title") ?? "") !== "")
    .map((b) => ({
      title: (b.getAttribute("title") ?? "").split(" · ")[0],
      indent: b.style.paddingLeft,
      dot: h.dot(b),
      unread: !!b.querySelector("[title='finished while you were elsewhere']"),
      pinned: !!b.querySelector("[title='pinned']"),
    }));

  window.__step = JSON.stringify({
    projects: Array.from(aside.querySelectorAll("section button"))
      .map((b) => b.textContent.trim())
      .filter((t) => t !== "" && t !== "+"),
    rows,
    hiddenProjectListed: aside.textContent.includes("omp-shell-archived"),
    emptyState: document.body.textContent.includes("No thread open"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # ------------------------------------------------------------- a resume
    checked(report, "resumeARow", harness, r"""
(async () => { try {
  const h = window.__helpers;
  h.click(h.row("Wire the sidebar"));
  await h.waitFor(() => (h.visible()?.textContent ?? "").includes("lists projects"));

  const opens = h.calls("open_thread").map((c) => c.args);
  window.__step = JSON.stringify({
    opens,
    transcript: h.visible()?.textContent.includes("the sidebar lists projects and their sessions"),
    dot: h.dot(h.row("Wire the sidebar")),
    // The row's own project, not the window's guess: the resume must spawn in the session's
    // recorded directory.
    workspace: opens[0]?.workspace ?? null,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # --------------------------------------------------- the composer's thread
    checked(report, "composerSpeaksToItsThread", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.type("second thought");
  await h.press("Enter");
  await h.waitFor(() => h.calls("prompt").length > 0, 4000);

  const prompts = h.calls("prompt").map((c) => c.args);
  window.__step = JSON.stringify({
    prompts,
    rowsGrew: (h.visible()?.textContent ?? "").includes("second thought"),
    cleared: h.box()?.value ?? null,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # ------------------------------------------- the palette, after the refactor
    checked(report, "paletteStillWorks", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.type("/");
  await h.waitFor(() => h.panel());
  const opened = !!h.panel();
  const rows = h.panel() ? h.panel().querySelectorAll("button").length : 0;
  await h.press("Escape");

  window.__step = JSON.stringify({ open: opened, rows, closed: !h.panel() });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # ------------------------------------------------- switching threads
    checked(report, "switchingThreads", harness, r"""
(async () => { try {
  const h = window.__helpers;
  h.click(h.row("Another project"));
  await h.waitFor(() => (h.visible()?.textContent ?? "").includes("hello from elsewhere"));

  const visible = h.visible()?.textContent ?? "";
  window.__step = JSON.stringify({
    opened: h.calls("open_thread").map((c) => c.args.resume),
    showsSecond: visible.includes("hello from elsewhere"),
    // The first thread's rows are still mounted — that is what keeps a background turn
    // landing — but they are not the column on screen.
    hidesFirst: !visible.includes("the sidebar lists projects and their sessions"),
    visibleColumns: Array.from(document.querySelectorAll("main > section"))
      .filter((s) => getComputedStyle(s).display !== "none").length,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # -------------------------------------------------------- an unread thread
    # ----------------------------------------------------------- diagnostics
    checked(report, "diagnosticsDrawer", harness, r"""
(async () => { try {
  const h = window.__helpers;
  h.click(h.action("diagnostics"));
  await h.sleep(400);

  const drawers = Array.from(document.querySelectorAll("aside"));
  const drawer = drawers[drawers.length - 1];
  const text = drawer?.textContent ?? "";
  window.__step = JSON.stringify({
    open: drawers.length > 1,
    negotiated: text.includes("negotiated v2") && text.includes("yes"),
    // The drawer describes the thread on screen, which by now is the second one.
    thread: text.includes("01a0shell0004"),
    pid: text.includes("4242"),
    counters: text.includes("11"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # -------------------------------------------- a thread with no file behind it
    checked(report, "aBrandNewThreadNeedsNoFile", harness, r"""
(async () => { try {
  const h = window.__helpers;
  h.click(document.querySelector("[data-row='new-thread']"));
  await h.waitFor(() => h.calls("open_thread").some((c) => c.args.resume === null));

  const rows = Array.from(document.querySelectorAll("aside button"))
    .filter((b) => (b.getAttribute("title") ?? "") !== "").length;
  window.__step = JSON.stringify({
    opened: h.calls("open_thread").filter((c) => c.args.resume === null).map((c) => c.args.workspace),
    rows,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # ------------------------------------------------- cross-thread search
    checked(report, "searchOverlay", harness, r"""
(async () => { try {
  const h = window.__helpers;
  // The window's own accelerator, not the sidebar button: both open it, and the key is how
  // it is normally reached (`docs/12` §1).
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true, bubbles: true }));
  await h.sleep(300);
  const open = !!h.searchPanel();

  const input = h.searchPanel()?.querySelector("input");
  input.value = "sidebar";
  input.dispatchEvent(new Event("input", { bubbles: true }));
  await h.sleep(500);

  const groups = Array.from(document.querySelectorAll("[aria-label='search every thread'] li button"))
    .map((b) => b.textContent.trim()).filter((t) => t !== "");
  const asked = h.calls("search").map((c) => c.args.query);

  window.__step = JSON.stringify({ open, asked, groups: groups.slice(0, 6) });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # A hit in another thread opens it and lands on the row, which is the whole point of an
    # index that read the *file* rather than the rendered transcript.
    checked(report, "searchJump", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.searchFor("hello");
  const hit = Array.from(h.searchPanel().querySelectorAll("li button"))
    .find((b) => b.textContent.includes("hello from elsewhere"));
  hit?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => (h.visible()?.textContent ?? "").includes("hello from elsewhere"), 4000);
  await h.sleep(300);

  const visible = h.visible()?.textContent ?? "";
  window.__step = JSON.stringify({
    // It was already live, so nothing was spawned: the shell switched to the thread it had.
    spawned: h.calls("open_thread").length,
    closed: !document.querySelector("[aria-label='search every thread']"),
    showsThread: visible.includes("hello from elsewhere"),
    // The flash marks the row the hit landed on, so the eye can find it in a long thread.
    flashed: document.querySelectorAll("main > section .ring-1").length,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "searchJumpToAToolCard", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.searchFor("rg sidebar");
  const hit = Array.from(h.searchPanel().querySelectorAll("li button"))
    .find((b) => b.textContent.includes("tool call"));
  hit?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => (h.visible()?.textContent ?? "").includes("bash"), 4000);
  await h.sleep(400);

  window.__step = JSON.stringify({
    searched: hit !== undefined,
    // The tool card is on screen and flashed: the index read the file, the row is the
    // transcript's own rendering of it, and the two were matched by content.
    flashed: document.querySelectorAll("main > section .ring-1").length,
    cardVisible: (h.visible()?.textContent ?? "").includes("rg sidebar src"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # ------------------------------------------------- the thread menu's flows
    checked(report, "renameInline", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.menu("Wire the sidebar");
  h.menuItem("Rename…")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(200);

  const input = h.row("Wire the sidebar")?.querySelector("input");
  const before = input?.value ?? null;
  input.value = "Renamed in the sidebar";
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
  await h.waitFor(() => h.row("Renamed in the sidebar"), 4000);

  window.__step = JSON.stringify({
    seeded: before,
    asked: h.calls("rename_thread").map((c) => c.args),
    // The engine answers a bare ack and emits nothing, so the new name is only on screen
    // because the catalogue was re-read.
    listed: !!h.row("Renamed in the sidebar"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # Pinning is dispatched through a *live* thread — its own or another — and shows up as
    # ordering, not as a flag the app keeps.
    checked(report, "pinReordersTheSidebar", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.menu("a session whose last turn was cut off");
  const pin = h.menuItem("Pin");
  pin?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => h.calls("pin_session").length > 0, 4000);
  await h.waitFor(() => (h.aside().textContent ?? "").indexOf("cut off") < (h.aside().textContent ?? "").indexOf("fork me"), 4000);

  // Only the thread rows: a project header carries its path as its title, and mixing the two
  // makes an order assertion meaningless.
  const order = Array.from(h.aside().querySelectorAll("button"))
    .map((b) => (b.getAttribute("title") ?? "").split(" · ")[0])
    .filter((t) => t !== "" && t.startsWith("a session") || t.startsWith("fork me") || t.startsWith("Wire") || t.startsWith("Renamed"));
  const reordered = (h.aside().textContent ?? "").indexOf("cut off") < (h.aside().textContent ?? "").indexOf("fork me");
  const calls = h.calls("pin_session").map((c) => c.args);
  window.__step = JSON.stringify({
    dispatchedThrough: calls[0]?.thread ?? null,
    pinned: calls[0]?.id ?? null,
    reordered,
    order: order.slice(0, 4),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # A fork mints a new session id in the same sidecar: the window has to *switch* to it.
    checked(report, "forkSwitchesThread", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.menu("fork me and continue");
  h.menuItem("Fork from here…")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => document.querySelector("[role='dialog'][aria-label='fork from a message']"), 4000);

  const panel = document.querySelector("[role='dialog'][aria-label='fork from a message']");
  const targets = Array.from(panel.querySelectorAll("li button")).map((b) => b.textContent.trim());
  const first = panel.querySelector("li button");
  first?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(200);
  const fork = Array.from(panel.querySelectorAll("button")).find((b) => (b.textContent ?? "").trim().startsWith("fork"));
  const state = { buttons: Array.from(panel.querySelectorAll("button")).map((b) => [b.textContent.trim().slice(0, 24), b.disabled]) };
  fork?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => h.calls("branch_thread").length > 0, 4000);
  await h.sleep(600);

  // The switch, observed the way a user would live it: type, and see which thread the words
  // went to. A sidebar row could be stale; the composer cannot.
  // Every open thread keeps a composer and only the active one is on screen, so *which* one
  // is visible is the switch. Nothing is typed: a send would only prove where focus was.
  const boxes = Array.from(document.querySelectorAll("textarea"));
  const visible = boxes.map((n, index) => [index, n.offsetParent !== null]).filter(([, shown]) => shown);

  window.__step = JSON.stringify({
    activeRow: Array.from(h.aside().querySelectorAll("button[class*='bg-ink-900']"))
      .map((b) => (b.getAttribute("title") ?? "").split(" · ")[0]),
    composers: Array.from(document.querySelectorAll("textarea")).map((t) => [t.disabled, t.offsetParent !== null]),
    found: !!h.row("forked thread"),
    asked: h.calls("branch_thread").map((c) => c.args),
    nested: (await (async () => {
      const { sidebarModel } = await import("/src/lib/threads.ts");
      const sessions = await window.__TAURI_INTERNALS__.invoke("sessions");
      const live = await window.__TAURI_INTERNALS__.invoke("threads");
      return sidebarModel({ sessions, live, unread: [], hidden: [], now: Date.now() })
        .flatMap((g) => g.threads.map((t) => t.id + "@" + t.depth));
    })()),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "exportOpensTheFile", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.menu("Another project");
  h.menuItem("Export HTML…")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => h.calls("open_path").length > 0, 4000);

  window.__step = JSON.stringify({
    exported: h.calls("export_html").map((c) => c.args.thread),
    opened: h.calls("open_path").map((c) => c.args.path),
    notice: (document.body.textContent ?? "").includes("exported to"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # Delete is the one destructive item, so it goes through a confirm and refuses a live
    # thread — both of which are the frontend's own rules.
    checked(report, "deleteNeedsAConfirmAndAColdThread", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.menu("forked thread");
  const item = h.menuItem("Delete session…");
  const refusedLive = item?.disabled ?? null;
  h.menuItem("Stop sidecar")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => h.calls("close_thread").length > 0, 4000);

  await h.menu("forked thread");
  h.menuItem("Delete session…")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => document.querySelector("[role='dialog'][aria-label='delete this session']"), 4000);
  const confirm = Array.from(document.querySelectorAll("[role='dialog'] button")).find((b) => (b.textContent ?? "").trim() === "delete");
  confirm?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => !h.row("forked thread"), 4000);

  window.__step = JSON.stringify({
    refusedLive,
    deleted: h.calls("delete_session").map((c) => c.args.id),
    gone: !h.row("forked thread"),
    // The confirmed delete is the only destructive path, and it says what it did.
    notice: (document.body.textContent ?? "").includes("deleted"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # ---------------------------------------------------- the right panel (docs/12 §8)
    checked(report, "panelShowsThePlan", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.show("Renamed in the sidebar");
  await h.showPanel();

  const text = h.panelText();
  window.__step = JSON.stringify({
    shown: !!h.threadPanel(),
    asides: Array.from(document.querySelectorAll("aside")).map((n) => [n.getAttribute("aria-label"), (n.textContent ?? "").replace(/\s+/g, " ").slice(0, 40)]),
    activeColumns: Array.from(document.querySelectorAll("textarea")).filter((t) => t.offsetParent !== null).length,
    // The three status marks the engine can send today, so a phase is legible at a glance. Read
    // off the icon rather than an attribute, and the icons name the *state* rather than the
    // drawing — a check that pinned the old glyphs is what this restyle broke.
    marks: {
      completed: !!h.threadPanel().querySelector("[data-icon='check']"),
      inProgress: !!h.threadPanel().querySelector("[data-icon='play']"),
      blocked: !!h.threadPanel().querySelector("[data-icon='alert']"),
    },
    progress: text.includes("1/3"),
    secondPhase: text.includes("Polish") && text.includes("0/1"),
    blocker: text.includes("waiting on the reviewer"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "panelTaskJump", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.show("Renamed in the sidebar");
  await h.showPanel();

  h.threadPanelButton("Read the workspace tree")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(600);

  const flashed = Array.from(document.querySelectorAll("[data-flash='on']"));
  window.__step = JSON.stringify({
    flashed: flashed.length,
    // The todo card is where the task was written, so that is where the jump has to land.
    landedOn: flashed.map((n) => (n.textContent ?? "").replace(/\s+/g, " ").slice(0, 70)),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "panelMarkDone", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.show("Renamed in the sidebar");
  await h.showPanel();
  const before = !!h.threadPanel().querySelector("[data-todo-mark='in_progress']");

  // The mark is the toggle: `set_todos` replaces the list, so the panel sends the whole plan
  // back with one status changed, and renders whatever the engine answers with.
  const mark = h.threadPanel().querySelector("[data-todo-mark='in_progress']");
  mark?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => h.calls("set_todos").length > 0, 4000);
  await h.sleep(800);

  const sent = (h.calls("set_todos")[0]?.args.phases ?? []);
  window.__step = JSON.stringify({
    wasInProgress: before,
    sentTasks: sent.reduce((total, phase) => total + phase.tasks.length, 0),
    sentStatuses: sent.flatMap((phase) => phase.tasks.map((task) => task.status)),
    // The answer is the engine's: the mark moved on without the panel assuming it would.
    nowCompleted: !h.threadPanel().querySelector("[data-todo-mark='in_progress']"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "panelChangedFiles", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.show("Renamed in the sidebar");
  await h.showPanel();
  h.threadPanelButton("Files")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(300);
  h.threadPanelButton("changed")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(300);
  const listed = h.panelText();

  h.threadPanelButton("diff")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(400);

  window.__step = JSON.stringify({
    // Relative to the thread's own workspace, because the window can hold several at once.
    relative: listed.includes("src/lib.rs"),
    absolute: listed.includes("/tmp/omp-shell-app/src/lib.rs"),
    oneRowForWriteThenEdit: (listed.match(/src\/lib\.rs/g) ?? []).length,
    touchedTwice: listed.includes("\u00d72"),
    bashNotListed: !listed.includes("seq 1 200000"),
    diffShown: h.panelText().includes("println"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "panelTreeUnfolds", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.show("Renamed in the sidebar");
  await h.showPanel();
  h.threadPanelButton("Files")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(300);
  h.threadPanelButton("tree")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(300);
  const beforeAsking = h.panelText();

  h.treeRow("workspace")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => h.calls("workspace_tree").length > 0, 4000);
  await h.sleep(400);
  const firstLevel = h.panelText();

  h.treeRow("src")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => h.calls("workspace_tree").length > 1, 4000);
  await h.sleep(400);

  window.__step = JSON.stringify({
    labelled: beforeAsking.includes("workspace"),
    // One level at a time, and the host is asked for the *thread's* own workspace.
    asked: h.calls("workspace_tree").map((c) => c.args.path),
    firstLevel: firstLevel.includes("README.md"),
    sizeShown: firstLevel.includes("2 KB"),
    unfoldedLazily: h.panelText().includes("lib.rs"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "panelArtifactReads", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.show("Renamed in the sidebar");
  await h.showPanel();
  h.threadPanelButton("Files")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(300);
  h.threadPanelButton("artifacts")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(300);
  const listed = h.panelText();

  h.threadPanelButton("artifact://7f3c")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => h.calls("read_artifact").length > 0, 4000);
  await h.sleep(500);
  const shown = h.panelText();

  window.__step = JSON.stringify({
    // The list is derived from the card that spilled, not from a directory listing.
    listedFromTheTruncatedCard: listed.includes("artifact://7f3c"),
    asked: h.calls("read_artifact").map((c) => c.args.id),
    text: shown.includes("the full output the card truncated"),
    // The file is 4 MB and the host caps what it returns: the panel says so.
    capped: shown.includes("4.0 MB"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "panelGoesReadOnly", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.show("Renamed in the sidebar");
  await h.showPanel();
  // The panel opens on the plan, but the check before this one left it on the Files tab.
  h.threadPanelButton("Todos")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(200);

  // Stop the thread's sidecar, which is the state `docs/12` §8.1 calls read-only: the engine
  // is holding no plan, and there is nothing to write to.
  const row = h.row("Renamed in the sidebar");
  row?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 100, clientY: 100 }));
  await h.sleep(200);
  Array.from(document.querySelectorAll("[role='menu'] button"))
    .find((b) => (b.textContent ?? "").trim().startsWith("Stop sidecar"))
    ?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => h.calls("close_thread").length > 0, 4000);
  await h.sleep(600);

  const text = h.panelText();
  // Every mark is a button, and read-only means every one of them is held down. Found by the
  // status each carries rather than by the glyph it draws.
  const marks = Array.from(h.threadPanel().querySelectorAll("[data-todo-mark]"));
  window.__step = JSON.stringify({
    // Read-only means the plan is still readable — the host remembered it when the thread left
    // the registry — and the write is what closes.
    explained: text.includes("No session is running"),
    planStillShown: text.includes("Read the workspace tree"),
    marks: marks.length,
    marksDisabled: marks.length > 0 && marks.every((b) => b.disabled),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # ---------------------------------------------------------- the agents panel
    checked(report, "agentsSidebarRow", harness, r"""
(async () => { try {
  const h = window.__helpers;
  // The count arrives from the host's answer to `agents` at startup, so the check waits for
  // the read rather than sampling whatever the first frame happened to show.
  await h.waitFor(() => window.__calls.some((c) => c.cmd === "agents"), 4000);
  await h.sleep(300);
  const row = h.agentsSidebarRow();
  const count = h.agentsSidebarCount();

  // A thread an earlier check stopped has gone, and its roster says `running` — the panel must
  // say what that means rather than count work no engine is doing.
  await h.openAgents();
  const text = h.agentsText();
  window.__step = JSON.stringify({
    row: (row?.textContent ?? "").trim(),
    count,
    title: row?.getAttribute("title") ?? null,
    // The sidebar's number and the panel's agree, and both are zero: the only running rows
    // belong to sessions whose engine has gone.
    panelTab: Array.from(document.querySelectorAll("[role='dialog'][aria-label^='Active agents'] button"))
      .map((b) => (b.textContent ?? "").trim())
      .find((label) => label.startsWith("agents")),
    closedExplained: text.includes("this session's engine is gone"),
    noRunningClaim: !text.includes("scoutrunning"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "agentsRoster", harness, r"""
(async () => { try {
  const h = window.__helpers;
  // The checks before this one stopped a sidecar, so the thread whose roster this asserts is
  // resumed first: a row in a thread whose engine has gone renders as exactly that, and the
  // live path is the one worth asserting.
  h.row("Renamed in the sidebar")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => (window.__world.live ?? []).some((thread) => thread.id === "01a0shell0001"), 4000);
  const runningRow = () => ({
    id: "scout", index: 0, agent: "scout", agentSource: "bundled", status: "running",
    description: null, task: "Find the sidebar wiring", assignment: null,
    sessionFile: "/tmp/omp-shell-app/sessions/shell/Wire.jsonl", parentToolCallId: "call_bg",
    detached: true, lastUpdateMs: Date.now(), listed: true,
    progress: {
      lastIntent: "reading the sidebar", currentTool: null, currentToolArgs: null,
      toolCount: 7, requests: 3, tokens: 4200, contextTokens: 1800, contextWindow: 200000,
      cost: 0.0125, durationMs: 9000, resolvedModel: null, resolvedThinkingLevel: null,
      advisor: false, retry: null, retryFailure: null,
    },
  });
  // Two rows, so the nesting is asserted as well: a child's transcript lives one directory
  // down, in a directory named after the agent that spawned it.
  const childRow = {
    id: "sonic", index: 1, agent: "sonic", agentSource: "bundled", status: "completed",
    description: null, task: "Rename the anchor", assignment: null,
    sessionFile: "/tmp/omp-shell-app/sessions/shell/scout/sonic.jsonl",
    parentToolCallId: "call_bg", detached: true, lastUpdateMs: Date.now(), listed: true,
    progress: null,
  };
  window.__fire("session-agents", { thread: "01a0shell0001", payload: [runningRow(), childRow] });
  await h.sleep(200);
  await h.openAgents();

  const text = h.agentsText();
  const running = h.agentRow("scout");
  const child = h.agentRow("sonic");
  const parked = h.agentRow("Older");
  const advisor = h.agentRow("advisor");
  const gone = h.agentRow("reviewer");

  // Nesting and the count are read off the DOM rather than the model: a child's indent is
  // what the panel renders, and an id in the text would prove nothing about it.
  const indent = (row) => parseFloat(getComputedStyle(row).paddingLeft) || 0;

  window.__step = JSON.stringify({
    text: text.slice(0, 400),
    // One running agent, and its finished child: a completed row is not in flight.
    tab: !!h.agentsTab("agents 1"),
    running: !!running && text.includes("scoutrunning"),
    stats: text.includes("$0.013") && text.includes("7 tools") && text.includes("3 req"),
    context: text.includes("1.8k / 200.0k"),
    nested: !!child && indent(child) > indent(running),
    parked: !!parked && text.includes("30498 bytes of transcript"),
    advisor: !!advisor && text.includes("advisor"),
    // The measured asymmetry: the engine stopped listing this one, and the panel says that
    // rather than picking a status.
    gone: !!gone && text.includes("stopped listing it"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "agentsTranscript", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.openAgents();
  h.agentRow("Older")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => h.agentsText().includes("older agent"), 4000);

  const text = h.agentsText();
  window.__step = JSON.stringify({
    asked: window.__calls.filter((c) => c.cmd === "agent_messages").map((c) => [c.args.thread, c.args.agent, c.args.fromByte]),
    read: text.includes("the older agent's last words"),
    // Which of the two readers answered is a claim the panel makes, so it is asserted: this
    // agent finished long ago, so only the file could have served it.
    fromFile: text.includes("the engine no longer serves this agent's transcript"),
    task: text.includes("no task recorded for it"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "agentsJump", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.openAgents();
  h.agentRow("scout")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => h.agentsText().includes("Find the sidebar"), 4000);

  const asked = h.agentsText().includes("Find the sidebar wiring");
  const button = Array.from(document.querySelectorAll("[role='dialog'][aria-label^='Active agents'] button"))
    .find((b) => (b.textContent ?? "").trim() === "show the task call");
  button?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(500);

  const flashed = Array.from(document.querySelectorAll("main [data-flash='on']")).length;
  window.__step = JSON.stringify({
    // The task text comes from the card the roster row belongs to, not from a label.
    asked,
    button: !!button,
    closed: h.agentsPanel() === null,
    flashed,
    cardVisible: (h.visible()?.textContent ?? "").includes("Backgrounded as job bg_7"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "agentsJobs", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.openAgents();
  h.agentsTab("jobs 1")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(300);

  const text = h.agentsText();
  window.__step = JSON.stringify({
    tab: !!h.agentsTab("jobs 1"),
    // `bg_7`'s card has no delivery, and `bg_2` has one: open first, delivered after.
    openJob: text.includes("bg_7") && text.includes("running"),
    delivered: text.includes("bg_2") && text.includes("delivered in 25.0s"),
    command: text.includes("sleep 25; echo done-sleeping"),
    // The engine exposes no job command, and the panel says so instead of implying one.
    honest: text.includes("no command that lists or cancels a background job"),
    broker: text.includes("vite") && text.includes("bun run dev"),
    owner: text.includes("started by"),
    stopShown: text.includes("stop"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "agentsStopProcess", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.openAgents();
  h.agentsTab("jobs 1")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(300);

  const stop = Array.from(document.querySelectorAll("[role='dialog'][aria-label^='Active agents'] button"))
    .find((b) => (b.textContent ?? "").trim() === "stop");
  stop?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => window.__calls.some((c) => c.cmd === "stop_broker_process"), 4000);
  await h.sleep(400);

  const text = h.agentsText();
  window.__step = JSON.stringify({
    asked: window.__calls.filter((c) => c.cmd === "stop_broker_process").map((c) => c.args.name),
    // The engine's own sentence, not one the panel wrote.
    notice: text.includes("Stopped vite"),
    // And the list is re-read rather than assumed: the stub flips the daemon to exited.
    reread: window.__calls.filter((c) => c.cmd === "broker_processes").length >= 2,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # ------------------------------------------------- the shell's own chrome
    checked(report, "theShellHasOneWayInAndItIsTheSidebar", harness, r"""
(async () => { try {
  const h = window.__helpers;

  // The titlebar is icon-only (`docs/12` §1): no directory field, no text buttons. What used to
  // be a path box there is a navigation act now, and it lives with the projects.
  const before = h.calls("open_thread").length;
  const opener = document.querySelector("[data-action='open-directory']");
  opener?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(200);

  const field = document.querySelector("[data-input='directory']");
  field.value = "/tmp/omp-typed-nowhere";
  field.dispatchEvent(new Event("input", { bubbles: true }));
  field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  await h.waitFor(() => h.calls("open_thread").length > before, 4000);

  const asked = h.calls("open_thread").slice(before).map((c) => c.args.workspace);

  window.__step = JSON.stringify({
    titlebarHasNoField: document.querySelector("header input") === null,
    titlebarIcons: Array.from(document.querySelectorAll("header [data-action]")).map((b) =>
      b.getAttribute("data-action"),
    ),
    // The typed path is exactly what reaches the host: the window does not guess or rewrite it,
    // which is the same contract the titlebar field had.
    asked,
    closed: document.querySelector("[data-input='directory']") === null,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "theIdentityRowSaysWhoIsHere", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.sleep(200);

  // The reference's sidebar footer is an identity row (`docs/12` §2.2). Ours is filled from the
  // launch context rather than invented, so what it shows is the OS user the host reported and
  // the size of the catalogue the window is already holding.
  const footer = document.querySelector("aside > div:last-child");
  const text = (footer?.textContent ?? "").replace(/\s+/g, " ").trim();
  const sessions = window.__world.sessions.length;

  window.__step = JSON.stringify({
    text,
    hasUser: text.includes("gio"),
    countsSessions: text.includes(String(sessions)),
    version: text.includes("v0.1.0"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "aFreshThreadIsGreeted", harness, r"""
(async () => { try {
  const h = window.__helpers;

  // A thread with messages is not greeted: the greeting is for the empty conversation
  // (`docs/12` §4), and it is keyed on the engine's own count so a *resuming* thread (rows not
  // yet read) cannot flash it.
  //
  // Scoped to the column on screen. Every open thread stays mounted, so a document-wide query
  // finds whichever thread has a greeting — which is how the first version of this check passed
  // while looking at the wrong thread entirely.
  // By its *current* title: an earlier check renames this thread, and a stale title meant this
  // check clicked nothing and then looked at whatever was already on screen — a thread with a
  // greeting, so it passed for the wrong reason until the scope was tightened and it failed for
  // the right one.
  await h.show("Renamed in the sidebar");
  const greetingOnALiveThread = !!h.visible().querySelector("[data-greeting]");

  const before = h.calls("open_thread").length;
  document.querySelector("[data-row='new-thread']")
    ?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => h.calls("open_thread").length > before, 4000);
  await h.sleep(600);

  const greeting = (document.querySelector("[data-greeting]")?.textContent ?? "")
    .replace(/\s+/g, " ")
    .trim();

  window.__step = JSON.stringify({
    // A false here is the assertion: a thread with messages has no greeting.
    greetingOnALiveThread,
    onAFreshThread: greeting,
    // The reference's shape: a time of day and who is sitting here, then a line of ours. The
    // hour is whatever it is when this runs, so the check pins the *shape*.
    namesTheUser: /(Late night|Morning|Afternoon|Evening), gio$/.test(greeting.split("A fresh")[0].trim()),
    subLine: greeting.includes("A fresh thread"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # ------------------------------------------------------------ the terminal drawer
    checked(report, "terminalOpensFromTheTitlebar", harness, r"""
(async () => { try {
  const h = window.__helpers;
  const shown = await h.showTerminals();

  // The emulator measures itself and tells the host, which is what makes a shell draw at the
  // window's size rather than at the size the host guessed when it allocated the pty.
  await h.waitFor(() => window.__world.terminalSizes.length > 0, 4000);
  const sized = window.__world.terminalSizes.at(-1);

  window.__step = JSON.stringify({
    shown,
    tabs: h.terminalTabs(),
    // The tab's own label is the directory's name, and its tooltip is the whole path.
    fullPath: h.terminalTab("omp-shell-app")?.getAttribute("title") ?? null,
    screenMounted: h.terminalScreen().length >= 0 && h.terminalInput() !== null,
    sized: { cols: (sized?.cols ?? 0) > 0, rows: (sized?.rows ?? 0) > 0 },
    readTheSet: h.calls("terminals").length >= 1,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "terminalShowsWhatTheHostPosts", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.showTerminals();

  await h.terminalOutput("pty-1", "hello from the shell\r\n");
  const first = h.terminalScreen();
  await h.terminalOutput("pty-1", "second line\r\n");
  const second = h.terminalScreen();

  window.__step = JSON.stringify({
    first: first.includes("hello from the shell"),
    // Batches accumulate rather than replace: the panel writes bytes into one emulator, so a
    // replay would show a terminal that only ever has the newest line.
    both: second.includes("hello from the shell") && second.includes("second line"),
    // Output for a tab this window does not have goes nowhere rather than into whatever is on
    // screen: one channel carries every tab, and routing by id is the whole safety property.
    strayStayedOut: (await h.terminalOutput("pty-404", "into the void\r\n"), !h.terminalScreen().includes("into the void")),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "terminalKeystrokesReachTheHost", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.showTerminals();
  const before = window.__world.terminalInput.length;
  await h.typeTerminal("ls");

  const typed = window.__world.terminalInput.slice(before);
  window.__step = JSON.stringify({
    typed: typed.map((entry) => entry.data).join(""),
    toTheRightTab: typed.every((entry) => entry.id === h.terminalActive()),
    // Two keys, two writes: the bytes are the emulator's own reading of the keys rather than
    // one message per keystroke batch.
    wentThroughXterm: typed.length === 2,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "terminalOpensInTheSessionsDirectory", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.waitFor(() => h.row("Wire the sidebar"));
  await h.show("Wire the sidebar");

  // Through the folder chip, which is the other way in (§5.1): the directory it opens in is
  // the *session's*, not the last one the window happened to use.
  //
  // Scoped to the column on screen: every open thread stays mounted (`v-show`), so a search of
  // `main button` finds the *first* thread's chip, which is how this check first reported
  // another session's directory and looked like a product bug.
  const folder = h.visible().querySelector("[data-chip='folder']");
  // The directory the chip *names*: the check reads it rather than hard-coding a fixture path,
  // so it holds whichever thread is on screen — and it is the real contract anyway, since a
  // terminal that opens anywhere else is exactly the bug this is here to catch.
  const named = (folder?.textContent ?? "").trim();
  folder?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(300);

  // Sentence case in the popover, and the check follows the copy rather than dictating it.
  const entry = Array.from(document.querySelectorAll("button")).find((b) =>
    (b.textContent ?? "").trim().toLowerCase().includes("open a terminal here"),
  );
  entry?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => h.calls("terminal_open").length > 0, 4000);
  await h.sleep(400);

  const asked = h.calls("terminal_open").map((c) => c.args);
  window.__step = JSON.stringify({
    asked: asked.map((args) => ({ cwd: args.cwd, cols: args.cols, rows: args.rows })),
    named,
    inTheSession: (asked[0]?.cwd ?? "").endsWith("/" + named) || asked[0]?.cwd === named,
    shown: h.terminalShown(),
    tabs: h.terminalTabs(),
    // The new tab is the one on screen: asking for a terminal and being left on another one
    // is the kind of thing that only shows up in a real window.
    selected: h.terminalActive(),
    newest: (window.__world.terminals ?? []).at(-1)?.id ?? null,
    onScreen: h.terminalActive() === ((window.__world.terminals ?? []).at(-1)?.id ?? null),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "terminalTabClosesAndTheSelectionMoves", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.showTerminals();
  const before = h.terminalTabs().length;

  // The close affordance is the × inside the tab, which is where a user's cursor goes.
  const close = h.terminalClose();
  close?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.waitFor(() => window.__world.terminalClosed.length > 0, 4000);
  await h.sleep(400);

  window.__step = JSON.stringify({
    closed: window.__world.terminalClosed,
    went: h.calls("terminal_close").map((c) => c.args.id),
    before,
    after: h.terminalTabs().length,
    // The host's own answer is what the strip renders — the window does not remove a tab
    // itself, because a shell it cannot see is still a shell.
    fromHost: (window.__world.terminals ?? []).length,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "settingsOpensFromTheSidebarAndShowsTheHostsNav", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.openSettings();

  const screen = h.settings();
  const nav = h.settingsNav().map((node) => node.getAttribute("data-section"));
  const sent = (window.__world.settings.screen.sections ?? []).map((section) => section.id);

  window.__step = JSON.stringify({
    opened: screen !== null,
    // The nav is the host's sections, in the host's order — the window does not add, drop or
    // reorder one, because the taxonomy is the mapping document's decision, not the layout's.
    nav,
    matchesHost: JSON.stringify(nav) === JSON.stringify(sent),
    // And the settings screen *replaces* the window's rail rather than stacking on it.
    threadRail: h.aside(),
    hasSearch: h.settingsSearch() !== null,
    askedFor: h.calls("settings_screen").length,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "aSettingRowWritesThroughTheHostAndSaysWhatItCosts", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.openSettings();

  // A toggle: the simplest write, and the one that must arrive with no confirmation.
  await h.showSection("model-and-providers");
  const before = h.calls("settings_write").length;
  h.settingRow("advisor.enabled")?.querySelector("[role='switch']")
    ?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(700);

  const writes = h.calls("settings_write").slice(before).map((call) => call.args);
  const wrote = writes[writes.length - 1] ?? {};
  const message = h.settingRow("advisor.enabled")?.querySelector("[data-message]")?.textContent ?? "";

  // The two restart classes are the promise a row makes, so they are asserted as a difference
  // in behaviour rather than in copy: a live row takes effect now and therefore offers no
  // restart, a sidecar row offers one.
  await h.showSection("shortcuts-and-keys");
  const live = h.settingRow("steeringMode");
  await h.showSection("general");
  const sidecar = h.settingRow("power.sleepPrevention");

  window.__step = JSON.stringify({
    wrote: wrote.key,
    value: wrote.value,
    confirmation: wrote.confirmation,
    // The host's own sentence is what the row shows afterwards, never a shorter claim.
    saysWhatItCost: message.includes("recorded"),
    liveClass: live?.getAttribute("data-restart"),
    sidecarClass: sidecar?.getAttribute("data-restart"),
    // The difference between the two classes, stated so neither value needs interpreting:
    // a live write is already in effect, so its row must not offer a restart at all.
    liveNeedsNoRestart: live?.querySelector("[data-action='restart-sessions']") == null,
    sidecarOffersRestart: sidecar?.querySelector("[data-action='restart-sessions']") != null,
    originShown: sidecar?.getAttribute("data-origin"),
  });
  await h.closeSettings();
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "aGatedRowIsVisibleDisabledAndNamesWhatWouldChangeIt", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.openSettings();
  h.navSection("model-and-providers")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(400);

  const row = h.settingRow("advisor.immuneTurns");
  const gated = row?.querySelector("[data-gated-control]");
  const namesIt = (gated?.textContent ?? "").includes("advisor.enabled");

  // §Q3 of the mapping: a gated row stays *visible* (a setting that vanishes reads as one that
  // never existed) and its control is replaced by the reason plus a way to the key that would
  // change it.
  const before = h.calls("settings_write").length;
  const link = gated?.querySelector("button");
  link?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(500);

  window.__step = JSON.stringify({
    visible: row !== null,
    disabled: gated !== null,
    namesTheKey: namesIt,
    // Following it puts the row that would change the condition on screen, and writes nothing.
    reachedTheKey: h.settingRow("advisor.enabled") != null,
    wrote: h.calls("settings_write").length - before,
    // And a row that cannot be written offers no restart: restarting sessions to apply a value
    // nobody could set is the kind of button that teaches people the screen is not literal.
    noRestartOnAGatedRow: row?.querySelector("[data-action='restart-sessions']") == null,
  });
  await h.closeSettings();
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "aCredentialIsReplaceOnlyAndTheFileNeverShowsOne", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.openSettings();
  h.navSection("memory")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(400);

  const row = h.settingRow("mnemopi.embeddingApiKey");
  const text = (row?.textContent ?? "").replace(/\s+/g, " ");
  // The row says it holds one and offers no way to read it back.
  const offersReveal = Array.from(row?.querySelectorAll("button") ?? [])
    .some((b) => /reveal|show|eye|peek/i.test(b.textContent ?? ""));

  // And the escape hatch, which shows the *file*, shows the mask where the value is: this is
  // the check that a credential cannot reach the webview through the one pane that prints YAML.
  await h.showHatch();
  const file = h.hatchEditor()?.value ?? "";
  const leaked = /sk-[A-Za-z0-9]|not-a-real-token|apiKey: [^•\s]/.test(file);

  window.__step = JSON.stringify({
    saysSet: /set|holds|••/.test(text),
    // Named for the expectation: there is no reveal path, because a stored credential is one
    // the app never reads back (`docs/13` §Secrets).
    noRevealControl: !offersReveal,
    // The pane shows the key (the user must know it is there) and the mask, never the value.
    fileHasTheKey: file.includes("embeddingApiKey"),
    fileMasked: file.includes("••••••••"),
    noLeakInTheFilePane: !leaked,
  });
  await h.closeSettings();
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "theHatchRefusesAPlanAndNeverAppliesIt", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.showHatch();

  // A value outside the engine's domain, which the host refuses by name.
  await h.hatchType("power:\n  sleepPrevention: nonsense\n");
  const before = h.calls("settings_hatch_apply").length;
  const apply = h.hatchAction("apply");
  const refused = (h.settings()?.textContent ?? "").includes("does not take");

  apply?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(600);

  window.__step = JSON.stringify({
    refusedShown: refused,
    // Disabled, not hidden-with-a-hope: the button exists, the user can see it, and it cannot
    // be pressed (`docs/13`'s protections: a plan with a refusal applies nothing).
    applyDisabled: apply?.disabled === true || apply?.getAttribute("aria-disabled") === "true",
    applied: h.calls("settings_hatch_apply").length - before,
    askedToPlan: h.calls("settings_hatch_plan").length > 0,
  });
  await h.closeSettings();
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "theHatchAsksForTheConfirmationTheHostDemands", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.showHatch();

  // `dev.autoqa` is on the do-not-surface list, so the host refuses any plan touching it
  // without the typed word. The window does not keep its own list of dangerous keys: it asks,
  // and the refusal *is* the signal.
  await h.hatchType("dev.autoqa: true\n");
  const before = h.calls("settings_hatch_apply").length;
  await h.clickAction(h.hatchAction("apply"));

  const asked = h.hatchConfirm() !== null;
  const first = h.calls("settings_hatch_apply").slice(before)[0]?.args ?? {};

  // Now the word, as a user would type it.
  const field = h.hatchConfirm();
  if (field) {
    field.value = "confirm";
    field.dispatchEvent(new Event("input", { bubbles: true }));
    await h.sleep(200);
  }
  await h.clickAction(h.hatchAction("apply"));

  const attempts = h.calls("settings_hatch_apply").slice(before).map((call) => call.args.confirmation);
  const applied = window.__world.settings.applied.length;

  window.__step = JSON.stringify({
    askedAfterTheRefusal: asked,
    firstAttemptWasUnconfirmed: first.confirmation === null,
    attempts,
    applied,
    reportShown: /applied|recorded|set\b/i.test(h.settings()?.textContent ?? ""),
  });
  await h.closeSettings();
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "theHatchShowsNoEditorForAFileItCannotParse", harness, r"""
(async () => { try {
  const h = window.__helpers;

  // The host deliberately sends no text for a file it cannot parse, so the pane must not offer
  // an empty editor that would overwrite it.
  window.__world.settings.hatch = {
    path: "/home/gio/.omp/agent/config.yml",
    exists: true,
    text: "",
    error: "line 2, column 6: mapping values are not allowed in this context",
    keys: [],
    refusals: [],
    backups: window.__world.settings.hatch.backups,
  };

  // Leave and come back, so the panel re-reads.
  h.navSection("general")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(200);
  await h.showHatch();

  const text = (h.settings()?.textContent ?? "").replace(/\s+/g, " ");
  window.__step = JSON.stringify({
    // No textarea at all: an empty one would offer to overwrite a file nobody can read.
    noEditor: h.hatchEditor() === null,
    showsTheError: text.includes("line 2, column 6"),
    explains: text.includes("cannot be read") || text.includes("cannot parse"),
    // The way out is still offered, and it is not "write anyway".
    offersReread: h.hatchAction("reread") !== null,
    offersBackup: h.hatchAction("backup") !== null,
  });
  await h.closeSettings();
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "restartingSessionsNamesTheOneItLeftAlone", harness, r"""
(async () => { try {
  const h = window.__helpers;

  // One session mid-turn and one idle: the host skips the first, and the report has to say so
  // rather than let the user believe a running turn was restarted under them.
  window.__world.live = [
    { id: "t-busy", workspace: "/tmp/omp-shell-app", title: "busy", streaming: true, pendingApprovals: 0, error: null },
    { id: "t-idle", workspace: "/tmp/omp-shell-other", title: "idle", streaming: false, pendingApprovals: 0, error: null },
  ];

  await h.openSettings();
  // A `sidecar` row, which is the class that needs the restart: a live key's row correctly
  // offers none (the check above asserts exactly that).
  await h.showSection("general");

  const row = h.settingRow("power.sleepPrevention");
  const action = row?.querySelector("[data-action='restart-sessions']");
  const before = h.calls("settings_restart_sessions").length;
  await h.clickAction(action);
  await h.sleep(400);

  // The report is the screen's own note: the restart is not one row's business.
  const said = (h.settings()?.textContent ?? "").replace(/\s+/g, " ");
  window.__step = JSON.stringify({
    offeredRestart: action != null,
    askedTheHost: h.calls("settings_restart_sessions").length - before,
    // Both halves reported: what came back, and what was deliberately left running.
    saysHowMany: /Restarted 1 session/.test(said),
    namesTheSkipped: said.includes("t-busy"),
  });
  await h.closeSettings();
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "settingsSearchFindsARowAcrossSections", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.openSettings();
  await h.showSection("general");

  // The field is in the rail for exactly this: 309 rows across fifteen sections is more than
  // anyone scrolls, and the row it finds may be in a section the user has never opened.
  const input = h.settingsSearch();
  const typed = input != null;
  if (input) {
    input.value = "sleep prevention";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await h.sleep(500);
  }

  const hit = Array.from(document.querySelectorAll("[data-view='settings'] button"))
    .find((b) => /sleep prevention/i.test(b.textContent ?? ""));
  hit?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(600);

  window.__step = JSON.stringify({
    hasField: typed,
    foundTheRow: hit != null,
    // Choosing a hit puts its section on screen — the outcome, not the highlight.
    landedOnTheRow: h.settingRow("power.sleepPrevention") != null,
    stillOpen: h.settings() != null,
  });
  await h.closeSettings();
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "terminalSaysWhoseShellItIs", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.showTerminals();

  // Close everything, so the empty state is what is on screen.
  for (let guard = 0; guard < 6 && h.terminalTabs().length > 0; guard += 1) {
    const close = h.terminalClose();
    close?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await h.sleep(400);
  }

  // §11 asks for the distinction in as many words: the panel is the user's shell, and the
  // agent's `bash` cannot see it. A window that stayed silent here would be read as "this is
  // what the tool cards are running in".
  const text = (h.terminalPanel()?.textContent ?? "").replace(/\s+/g, " ");
  window.__step = JSON.stringify({
    tabs: h.terminalTabs().length,
    saysYours: text.includes("your") && text.includes("not the agent's"),
    saysNoPty: text.includes("cannot see or drive"),
    offersOne: text.includes("opens a shell in this workspace"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "terminalThatExitedTakesNoInput", harness, r"""
(async () => { try {
  const h = window.__helpers;
  await h.showTerminals();

  // The host announces an exit the way it really does: the row stays, with the way it ended.
  window.__fire("terminals-updated", [
    { id: "pty-9", cwd: "/tmp/omp-shell-other", running: false, exit: { code: 0, signal: null }, pid: 5150 },
  ]);
  await h.sleep(400);

  const before = window.__world.terminalInput.length;
  await h.typeTerminal("exit");
  const typed = window.__world.terminalInput.length - before;

  window.__step = JSON.stringify({
    tabs: h.terminalTabs(),
    saysHow: (h.terminalPanel()?.textContent ?? "").includes("exited 0"),
    // A shell that has gone takes no keys: nothing reaches the host, because a terminal that
    // accepts typing into nothing is a lie about what is running.
    typedWhileDead: typed,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "agentsCountFollowsEvents", harness, r"""
(async () => { try {
  const h = window.__helpers;
  // The count is fed by the host's own stream rather than by opening anything: a subagent
  // starting while the window is looking elsewhere is the case the row exists for. Fired for a
  // thread this check knows is live, because a closed thread's roster must *not* count.
  const liveThread = (window.__world.live ?? [])[0]?.id;
  const running = (id, index) => ({
    id, index, agent: "scout", agentSource: "bundled", status: "running",
    description: null, task: null, assignment: null, sessionFile: null,
    parentToolCallId: null, detached: true, lastUpdateMs: Date.now(), listed: true, progress: null,
  });

  window.__fire("session-agents", {
    thread: liveThread,
    payload: [running("One", 0), running("Two", 1), running("Three", 2)],
  });
  await h.sleep(200);
  const withEvent = h.agentsSidebarCount();

  // The same state, counted by the panel: two renderings of one number, so a disagreement is
  // a bug whichever one is right.
  await h.openAgents();
  const panelTab = Array.from(document.querySelectorAll("[role='dialog'][aria-label^='Active agents'] button"))
    .map((b) => (b.textContent ?? "").trim())
    .find((label) => label.startsWith("agents"));
  window.__step = JSON.stringify({
    liveThread,
    withEvent,
    panelTab,
    agree: panelTab === `agents ${withEvent}`,
    fired: window.__fired.filter((entry) => entry.event === "session-agents").length,
    sidebar: (h.agentsSidebarRow()?.textContent ?? "").trim(),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # ------------------------------------------- notifications (`docs/12` §13)
    #
    # This group starts from a *fresh* window. The checks before it open panels, screens and
    # drawers that share the window with the sidebar, and a group whose subject is the
    # notification rules should not also be a test of whatever was left on screen — an earlier
    # version of these checks failed for exactly that reason, and reported it as a product bug.
    # Reloading resets the mock host's world too, so the `claim` helper below can prove that the
    # thread it needs is genuinely live before anything is asserted about it.
    harness.reload()
    harness.js(PRELUDE)

    #
    # The four triggers, and the one rule that decides whether any of them becomes a banner: a
    # person who is looking at the thread is not interrupted, and a person who is not is told.
    # Every assertion is either what the window drew or what it asked the host to do.

    checked(report, "theWindowSaysWhichThreadIsOnScreen", harness, r"""
(async () => { try {
  const h = window.__helpers;
  const live = await h.claim("Wire the sidebar", "01a0shell0001");
  const first = window.__world.focusedThread;

  await h.claim("fork me and continue", "01a0shell0002");
  const second = window.__world.focusedThread;

  window.__step = JSON.stringify({
    live,
    // The idle policy releases a quiet sidecar after ten minutes. This is the one guard it
    // cannot derive: which thread a person is reading, so it must not be released under them.
    firstNamesTheThreadItOpened: first === "01a0shell0001",
    movedWithTheWindow: second === "01a0shell0002",
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "aTurnEndingInAnotherThreadIsAnnouncedInTheHostsOwnWords", harness, r"""
(async () => { try {
  const h = window.__helpers;
  const live = await h.claim("Wire the sidebar", "01a0shell0001");
  h.focus(false);                                  // nobody is looking at the window
  const before = h.banners().length;
  window.__fire("session-notifications", {
    thread: "01a0shell0004", kind: "turn-finished",
    title: "Another project", body: "The turn finished after three tools.",
    jobId: null,
  });
  await h.sleep(350);

  const sent = h.banners().slice(before);
  const banner = sent[0] ?? {};
  const toast = h.toasts()[0];
  window.__step = JSON.stringify({
    live,
    // One event, one ask: a listener registered twice would show up here as a double banner.
    banners: sent.length,
    viaTheHost: sent.length > 0 && sent.every((entry) => entry.via === "host"),
    // The window is a window, not a summariser: what the banner says is the host's sentence.
    bannerIsTheHosts: String(banner.body ?? "").includes("finished after three tools"),
    toastSays: h.text(toast),
    toastIsTheHosts: h.text(toast).includes("finished after three tools"),
    markedUnread: !!h.row("Another project")?.querySelector("[title='finished while you were elsewhere']"),
  });
  h.click(toast?.querySelector("[data-action='dismiss-notice']") ?? null);
  await h.sleep(150);
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "theThreadOnScreenIsNeverAnnounced", harness, r"""
(async () => { try {
  const h = window.__helpers;
  const live = await h.claim("Wire the sidebar", "01a0shell0001");   // the thread on screen
  h.focus(false);                                  // and the window is out of sight
  const before = h.banners().length;
  const toastsBefore = h.toasts().length;
  window.__fire("session-notifications", {
    thread: "01a0shell0001", kind: "turn-finished",
    title: "Wire the sidebar", body: "The turn finished.", jobId: null,
  });
  await h.sleep(350);
  window.__step = JSON.stringify({
    live,
    banners: h.banners().length - before,
    toasts: h.toasts().length - toastsBefore,
    unread: !!h.row("Wire the sidebar")?.querySelector("[title='finished while you were elsewhere']"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "aBannerTheHostCannotShowStillLeavesTheNotice", harness, r"""
(async () => { try {
  const h = window.__helpers;
  const live = await h.claim("Wire the sidebar", "01a0shell0001");
  h.focus(false);
  window.__world.notifyFails = true;               // the platform refused the banner
  const attemptsBefore = window.__world.notifyAttempts.length;
  const errorsBefore = window.__errors.length;

  window.__fire("session-notifications", {
    thread: "01a0shell0004", kind: "failed",
    title: "Another project", body: "The turn failed: the provider refused.", jobId: null,
  });
  await h.sleep(400);

  window.__step = JSON.stringify({
    live,
    // It was tried, so the window did not decide a banner was not worth asking for.
    tried: window.__world.notifyAttempts.length - attemptsBefore,
    shown: window.__world.notifications.filter((entry) => String(entry.body ?? "").includes("provider refused")).length,
    // The notice is the delivery that cannot fail, so a refused banner costs nothing but the
    // banner — the person still finds out, which is the whole reason it is in-app first.
    noticeShown: h.toasts().some((toast) => h.text(toast).includes("provider refused")),
    // And the window lives with the refusal instead of failing loudly into a console nobody
    // is watching: an unhandled rejection here is exactly the silent-failure class this step
    // exists to close.
    unhandled: window.__errors.length - errorsBefore,
  });

  window.__world.notifyFails = false;
  for (const toast of h.toasts()) h.click(toast.querySelector("[data-action='dismiss-notice']"));
  await h.sleep(150);
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "aNoticeOpensItsThreadAndClearsItsOwnMark", harness, r"""
(async () => { try {
  const h = window.__helpers;
  const live = await h.claim("Wire the sidebar", "01a0shell0001");
  h.focus(false);
  window.__fire("session-notifications", {
    thread: "01a0shell0004", kind: "turn-finished",
    title: "Another project", body: "Finished in another project.", jobId: null,
  });
  await h.sleep(250);
  window.__fire("session-notifications", {
    thread: "01a0shell0003", kind: "job-finished",
    title: "Cut off", body: "Background job 7 finished.", jobId: "7",
  });
  await h.sleep(250);

  const stacked = h.toasts().length;
  const second = h.toasts()[1];
  const openedBefore = h.calls("open_thread").length;
  h.click(second?.querySelector("[data-action='open-notice']"));
  await h.sleep(700);

  window.__step = JSON.stringify({
    live,
    // Two events, two notices: a second one must not replace the first.
    stacked,
    // The notice opened *that* thread — asserted where it is observable, which is the call
    // the window made, not the fixture's transcript (this one has no rows to show).
    opened: h.calls("open_thread").slice(openedBefore).some((call) => call.args?.resume === "01a0shell0003"),
    itsNoticeGone: h.toasts().length === stacked - 1,
    // Opening it is what makes it read: the mark goes with the notice.
    markCleared: !h.row("a session whose last turn was cut off")?.querySelector("[title='finished while you were elsewhere']"),
    jobNamed: h.text(second).includes("Background job 7"),
  });
  for (const toast of h.toasts()) h.click(toast.querySelector("[data-action='dismiss-notice']"));
  await h.sleep(150);
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # ------------------------------ extension chrome (`docs/12` §14 item 4)
    #
    # The engine's fire-and-forget UI family: a status line, a widget, a title, editor text, a
    # notice and a URL. None of them may leak across threads — that is the whole risk, since
    # every one of them is a host-side fact about one session.

    checked(report, "anExtensionsStatusLineAndWidgetBelongToTheirThread", harness, r"""
(async () => { try {
  const h = window.__helpers;
  const live = await h.claim("Wire the sidebar", "01a0shell0001");
  window.__fire("session-chrome", { thread: "01a0shell0001", op: { kind: "status", text: "indexing 42 files" } });
  await h.sleep(200);
  const statusShown = h.text(h.statusLine());
  window.__fire("session-chrome", { thread: "01a0shell0001", op: { kind: "widget", lines: ["build: ok", "tests: 12"] } });
  await h.sleep(200);
  const widgetShown = h.text(h.widget());

  // For another thread: neither may appear here.
  window.__fire("session-chrome", { thread: "01a0shell0004", op: { kind: "status", text: "somewhere else entirely" } });
  await h.sleep(250);
  const leakedStatus = h.text(h.statusLine());
  window.__fire("session-chrome", { thread: "01a0shell0004", op: { kind: "widget", lines: ["not my thread"] } });
  await h.sleep(250);
  const leakedWidget = h.text(h.widget());

  // A null clears, which is how the engine takes its own chrome away.
  window.__fire("session-chrome", { thread: "01a0shell0001", op: { kind: "status", text: null } });
  window.__fire("session-chrome", { thread: "01a0shell0001", op: { kind: "widget", lines: null } });
  await h.sleep(250);

  window.__step = JSON.stringify({
    live,
    statusShown,
    widgetShown,
    // A missing element and an empty one are different failures, so say which.
    statusElement: h.statusLine() !== null,
    widgetElement: h.widget() !== null,
    noLeakInStatus: !leakedStatus.includes("somewhere else entirely"),
    noLeakInWidget: !leakedWidget.includes("not my thread"),
    cleared: h.statusLine() === null && h.widget() === null,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "editorTextLandsOnlyInTheThreadItNames", harness, r"""
(async () => { try {
  const h = window.__helpers;
  const live = await h.claim("Wire the sidebar", "01a0shell0001");
  await h.type("a draft of my own");
  await h.sleep(150);
  const mine = h.composerValue();

  // Another thread's text must not touch this composer — the engine pushes editor text when a
  // session sets one, and a draft is the one thing a person would notice losing.
  window.__fire("session-chrome", { thread: "01a0shell0004", op: { kind: "editor-text", text: "from somewhere else" } });
  await h.sleep(300);
  const untouched = h.composerValue();

  // Its own thread's text does land, because that is what the engine asked for.
  window.__fire("session-chrome", { thread: "01a0shell0001", op: { kind: "editor-text", text: "the engine's own text" } });
  await h.sleep(300);
  const landed = h.composerValue();

  window.__step = JSON.stringify({
    live,
    mine,
    untouchedIsMine: untouched === mine,
    landed,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "anExtensionNoticeIsShownWithoutABannerWhileFocused", harness, r"""
(async () => { try {
  const h = window.__helpers;
  const live = await h.claim("Wire the sidebar", "01a0shell0001");
  h.focus(true);                                   // the window is in front of someone
  const before = h.banners().length;
  window.__fire("session-chrome", { thread: "01a0shell0001", op: { kind: "notify", message: "logged in as gio", level: "info" } });
  await h.sleep(300);
  window.__step = JSON.stringify({
    live,
    notice: h.text(h.toasts().find((toast) => h.text(toast).includes("logged in as gio"))),
    banners: h.banners().length - before,
  });
  for (const toast of h.toasts()) h.click(toast.querySelector("[data-action='dismiss-notice']"));
  await h.sleep(150);
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "anOpenUrlGoesThroughTheHostsOwnOpener", harness, r"""
(async () => { try {
  const h = window.__helpers;
  const live = await h.claim("Wire the sidebar", "01a0shell0001");
  window.__fileDoorBefore = h.calls("open_path").length;
  const before = h.calls("open_external").length;
  window.__fire("session-chrome", { thread: "01a0shell0001", op: { kind: "open-url", url: "https://omp.sh/docs" } });
  await h.sleep(350);
  const opened = h.calls("open_external").slice(before);
  window.__step = JSON.stringify({
    live,
    // One path to the outside, and it is the host's: `open_external` is where the scheme
    // allowlist lives, and it is deliberately not `open_path`'s door, which admits paths the
    // app itself owns.
    asked: opened.length,
    url: opened[0]?.args?.url ?? null,
    // And it never arrives through the file opener.
    viaTheFileDoor: h.calls("open_path").length - window.__fileDoorBefore,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    # ------------------------------- idle suspension (`docs/11` §3.1, decision D5)
    #
    # The host releases the process; the window's part is to say so and to bring the thread back
    # through the path that already exists. The order matters: the row first, the resume second.

    checked(report, "aThreadWhoseSidecarWasReleasedKeepsItsRowAndSaysSo", harness, r"""
(async () => { try {
  const h = window.__helpers;
  // The thread has to be *live* first: the host suspends the sessions it holds, and the window
  // learns about it from the live set going quiet. Flipping the catalogue flag alone would
  // test a state the host can never produce.
  const live = await h.claim("Wire the sidebar", "01a0shell0001");
  const row = h.row("Wire the sidebar");
  const liveDot = h.dot(row);
  const opensBefore = h.calls("open_thread").length;

  window.__suspend("01a0shell0001");               // the host releases the process
  await h.sleep(600);
  const after = h.row("Wire the sidebar");

  window.__step = JSON.stringify({
    live,
    keptItsRow: after !== null,
    saysSuspended: (after?.textContent ?? "").includes("suspended"),
    dotChanged: h.dot(after) !== liveDot,
    // Releasing a process is the host's own act: the window must not answer it by opening
    // anything, which would spawn the sidecar straight back.
    openedByTheWindow: h.calls("open_thread").length - opensBefore,
    claimsNothingLive: !(after?.getAttribute("title") ?? "").includes("streaming"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "clickingASuspendedRowBringsTheConversationBack", harness, r"""
(async () => { try {
  const h = window.__helpers;
  const before = h.calls("open_thread").length;
  const suspendedRow = h.row("Wire the sidebar") ?? h.rowByText("Wire the sidebar");
  h.click(suspendedRow);
  await h.sleep(900);

  const opened = h.calls("open_thread").slice(before);
  const row = h.row("Wire the sidebar");
  const conversation = (h.visible()?.textContent ?? "").replace(/\s+/g, " ");
  window.__step = JSON.stringify({
    asked: opened.length,
    // The existing open path, unchanged: the session id it wants resumed and the workspace it
    // recorded. The host resolves that id to a session file itself, so nothing new is asked.
    resumed: opened[0]?.args?.resume ?? null,
    workspace: opened[0]?.args?.workspace ?? null,
    labelGone: !(row?.textContent ?? "").includes("suspended"),
    dotBack: h.dot(row),
    // Lossless in the window's terms: the conversation it had is on screen again.
    transcriptBack: conversation.includes("wire the sidebar"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "anIndexThatCouldNotBeRebuiltSaysWhyInsteadOfNothing", harness, r"""
(async () => { try {
  const h = window.__helpers;
  const closeIt = () => document
    .querySelector("[role='dialog'] button[title='close']")
    ?.dispatchEvent(new MouseEvent("click", { bubbles: true }));

  await h.openSearch();
  const quiet = h.text(h.searchPanel());
  closeIt();
  await h.sleep(250);

  // A pass that could not finish. "no sessions indexed yet" is the same words for an empty
  // store and for an index nobody could read, and only one of those is true.
  window.__world.indexError = "the search index could not be opened: the file is not a database";
  await h.openSearch();
  const said = h.text(h.searchPanel());

  window.__step = JSON.stringify({
    quietSaysNothingAboutFailures: !quiet.includes("could not be opened"),
    saysTheReason: said.includes("the file is not a database"),
    doesNotAlsoClaimEmptiness: !said.includes("no sessions indexed yet"),
  });

  window.__world.indexError = null;
  closeIt();
  await h.sleep(200);
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "theLoginRowOpensATerminalRunningTheEnginesOwnFlow", harness, r"""
(async () => { try {
  const h = window.__helpers;
  const live = await h.claim("Wire the sidebar", "01a0shell0001");

  await h.type("/log-in");
  await h.waitFor(() => h.panel());
  const row = Array.from(h.panel()?.querySelectorAll("button") ?? [])
    .find((b) => (b.textContent ?? "").includes("Log in to a provider"));

  const before = h.calls("terminal_open").length;
  row?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await h.sleep(800);

  const opened = h.calls("terminal_open").slice(before);
  window.__step = JSON.stringify({
    live,
    rowExists: row !== null,
    // One tab, in the workspace the thread on screen is in, for the engine's own flow.
    tabs: opened.length,
    cwd: opened[0]?.args?.cwd ?? null,
    inTheWorkspace: opened[0]?.args?.cwd === "/tmp/omp-shell-app",
    // Spelled the way `docs/04` says it exists: `omp login` is not a command, `/login` is
    // TUI-only, and the vault's OAuth flow is `omp auth-broker login`.
    typedTheEnginesOwnFlow: window.__world.terminalInput
      .some((entry) => String(entry.data ?? "").includes("omp auth-broker login")),
    // The row was taken, not left open behind the tab it opened.
    paletteClosed: h.panel() === null,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    checked(report, "aTitleFromTheEngineRenamesTheRow", harness, r"""
(async () => { try {
  const h = window.__helpers;
  const live = await h.claim("Wire the sidebar", "01a0shell0001");
  window.__fire("session-chrome", { thread: "01a0shell0001", op: { kind: "title", title: "Sidebar, resumable" } });
  await h.sleep(350);
  const stale = Array.from(h.aside()?.querySelectorAll("button") ?? [])
    .filter((b) => (b.textContent ?? "").includes("Wire the sidebar"))
    .map((b) => ({ text: (b.textContent ?? "").trim().slice(0, 40), title: (b.getAttribute("title") ?? "").slice(0, 40) }));

  window.__step = JSON.stringify({
    live,
    // By the label the row *renders*: a tooltip is a different surface from the title, and
    // asserting on it would fail a row that renamed correctly.
    renamed: h.rowByText("Sidebar, resumable") !== null,
    // The engine renamed the session; the stale name must not survive — and if it does, say
    // which row is carrying it rather than reporting a bare false.
    oldGone: stale.length === 0,
    staleRows: stale,
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")

    return report
