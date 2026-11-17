import base64
import json
from pathlib import Path

from window import PRELUDE, checked


def run(harness):
    report = {}
    harness.js(PRELUDE)
    checked(report, "attachmentConversationIsReady", harness, r"""
(async () => { try {
  const h = window.__helpers;
  const project = await h.waitFor(() => h.all('aside [role="button"]')
    .find(element => element.textContent.includes("omp-shell-app")), 4000);
  if (!h.row("Wire the sidebar") && project) {
    project.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, button: 0 }));
    await h.sleep(100);
  }
  if (!await h.claim("Wire the sidebar", "01a0shell0001")) throw new Error("No fixture conversation");
  h.button("Got it")?.click();
  window.__step = JSON.stringify({ ok: !!h.visible()?.querySelector("textarea") });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")
    icon = Path(__file__).resolve().parents[2] / "src-tauri/icons/32x32.png"
    encoded = base64.b64encode(icon.read_bytes()).decode("ascii")
    harness.js("window.__dropImage = " + json.dumps(encoded) + "; true")
    harness.js(r"""
window.isTauri = true;
window.__originalDropInvoke = window.__TAURI_INTERNALS__.invoke;
window.__TAURI_INTERNALS__.invoke = async (command, args) => {
  const result = await window.__originalDropInvoke(command, args);
  return command === "read_file_base64" && args.path.endsWith("drop-image.png")
    ? window.__dropImage : result;
};
window.__dropChecks = {
  target: () => window.__helpers.visible(),
  point() {
    const bounds = this.target().getBoundingClientRect();
    return { x: bounds.left + 12, y: bounds.top + 24 };
  },
  drop(paths, position = this.point()) {
    window.__fire("tauri://drag-drop", { paths, position });
  },
  has(name, target = this.target()) {
    return target.textContent.includes(name);
  },
  html(name) {
    const transfer = new DataTransfer();
    transfer.items.add(new File(["local text"], name, { type: "text/plain" }));
    this.target().dispatchEvent(new DragEvent("drop", {
      bubbles: true, cancelable: true, dataTransfer: transfer,
    }));
  },
};
true
""")
    checked(report, "nativeDropUsesItsOwnLogicalPositionAfterLeaveAtHiDpi", harness, r"""
(async () => { try {
  const h = window.__helpers, d = window.__dropChecks;
  const descriptor = Object.getOwnPropertyDescriptor(window, "devicePixelRatio");
  try {
    Object.defineProperty(window, "devicePixelRatio", { configurable: true, value: 2 });
    window.__fire("tauri://drag-leave", {});
    d.drop(["/tmp/drop-image.png"]);
    const image = await h.waitFor(() => d.target().querySelector('img[alt="drop-image.png"]'), 2000);
    await h.waitFor(() => image?.complete && image.naturalWidth > 0, 1000);
    window.__step = JSON.stringify({
      ok: !!image && image.complete && image.naturalWidth > 0,
      thumbnail: !!image, width: image?.naturalWidth,
    });
  } finally {
    if (descriptor) Object.defineProperty(window, "devicePixelRatio", descriptor);
    else delete window.devicePixelRatio;
  }
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")
    checked(report, "twoSeparateNativeDropsAreNeverThrottled", harness, r"""
(async () => { try {
  const h = window.__helpers, d = window.__dropChecks;
  d.drop(["/tmp/first-drop.txt"]);
  d.drop(["/tmp/second-drop.txt"]);
  await h.sleep(40);
  window.__step = JSON.stringify({
    ok: d.has("first-drop.txt") && d.has("second-drop.txt"),
    first: d.has("first-drop.txt"), second: d.has("second-drop.txt"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")
    checked(report, "dropsOutsideChatAndDuplicateDomDeliveryDoNotAttach", harness, r"""
(async () => { try {
  const h = window.__helpers, d = window.__dropChecks;
  d.drop(["/tmp/outside-chat.txt"], { x: 10, y: 100 });
  d.html("duplicate-dom.txt");
  await h.sleep(40);
  window.__step = JSON.stringify({
    ok: !d.has("outside-chat.txt") && !d.has("duplicate-dom.txt"),
  });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")
    checked(report, "browserFileDropStillAttachesTextContent", harness, r"""
(async () => { try {
  const h = window.__helpers, d = window.__dropChecks;
  window.isTauri = false;
  try {
    d.html("browser-file.txt");
    await h.waitFor(() => d.has("browser-file.txt"), 1000);
    window.__step = JSON.stringify({ ok: d.has("browser-file.txt") });
  } finally {
    window.isTauri = true;
  }
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")
    checked(report, "onlyTheVisibleThreadReceivesADrop", harness, r"""
(async () => { try {
  const h = window.__helpers, d = window.__dropChecks;
  const background = d.target();
  await h.show("fork me and continue");
  d.drop(["/tmp/active-thread-only.txt"]);
  await h.sleep(40);
  const active = d.has("active-thread-only.txt");
  const untouched = !d.has("active-thread-only.txt", background);
  await h.show("Wire the sidebar");
  const retained = d.has("drop-image.png") && d.has("first-drop.txt") && d.has("second-drop.txt");
  window.__step = JSON.stringify({ ok: active && untouched && retained, active, untouched, retained });
} catch (error) { window.__step = JSON.stringify({ error: String(error) }); } })()
""")
    return report
