import { afterEach, describe, expect, test } from "bun:test";
import { createApp, defineComponent, h, nextTick, ref } from "vue";
import { useSelectionStableText } from "./selectionStableText";

const cleanups: Array<() => void> = [];

function mountBody(initial: string) {
  const text = ref(initial);
  const rootRef = ref<HTMLElement | null>(null);
  const Body = defineComponent({
    setup() {
      const stable = useSelectionStableText(() => text.value, () => rootRef.value);
      return () => h("div", { ref: rootRef, innerHTML: stable.value });
    },
  });
  const host = document.createElement("div");
  document.body.append(host);
  const app = createApp(Body);
  app.mount(host);
  cleanups.push(() => {
    app.unmount();
    host.remove();
  });
  return { text, root: host.firstElementChild as HTMLElement };
}

function paragraphs(root: HTMLElement): [HTMLElement, HTMLElement] {
  const [first, second] = Array.from(root.children) as [HTMLElement, HTMLElement];
  return [first, second];
}

function selectText(
  anchor: Node,
  anchorOffset: number,
  focus: Node = anchor,
  focusOffset = anchorOffset + 1,
): Selection {
  const selection = window.getSelection();
  if (!selection) throw new Error("no document selection");
  selection.setBaseAndExtent(anchor, anchorOffset, focus, focusOffset);
  return selection;
}

function selectionChanged(): void {
  document.dispatchEvent(new window.Event("selectionchange"));
}

afterEach(() => {
  while (cleanups.length) cleanups.pop()!();
  window.getSelection()?.removeAllRanges();
  document.body.replaceChildren();
});

describe("useSelectionStableText", () => {
  test("holds rendered nodes while selected, coalesces updates, then shows the latest", async () => {
    const body = mountBody("<p>first</p><p>second</p>");
    const [first, second] = paragraphs(body.root);
    const selection = selectText(first.firstChild!, 1, second.firstChild!, 5);

    body.text.value = "<p>first one</p><p>second</p>";
    await nextTick();
    expect(body.root.firstElementChild).toBe(first);
    expect(body.root.innerHTML).toBe("<p>first</p><p>second</p>");

    body.text.value = "<p>first one</p><p>second two</p>";
    await nextTick();
    expect(body.root.innerHTML).toBe("<p>first</p><p>second</p>");

    selection.removeAllRanges();
    selectionChanged();
    await nextTick();
    expect(body.root.innerHTML).toBe("<p>first one</p><p>second two</p>");
  });

  test("keeps a neighbouring body streaming while the selection sits elsewhere", async () => {
    const selected = mountBody("<p>mine</p>");
    const other = mountBody("<p>theirs</p>");
    selectText(selected.root.firstChild!.firstChild!, 1);

    selected.text.value = "<p>mine next</p>";
    other.text.value = "<p>theirs next</p>";
    await nextTick();

    expect(selected.root.innerHTML).toBe("<p>mine</p>");
    expect(other.root.innerHTML).toBe("<p>theirs next</p>");
  });

  test("holds when the selection spans the body from neighbouring blocks", async () => {
    const before = document.createElement("p");
    before.textContent = "before";
    document.body.append(before);
    const body = mountBody("<p>middle</p>");
    const after = document.createElement("p");
    after.textContent = "after";
    document.body.append(after);

    selectText(before.firstChild!, 2, after.firstChild!, 2);

    body.text.value = "<p>middle next</p>";
    await nextTick();
    expect(body.root.innerHTML).toBe("<p>middle</p>");

    selectionChanged();
    await nextTick();
    expect(body.root.innerHTML).toBe("<p>middle</p>");

    window.getSelection()!.removeAllRanges();
    selectionChanged();
    await nextTick();
    expect(body.root.innerHTML).toBe("<p>middle next</p>");
  });

  test("shows the latest after the selection moves to another body", async () => {
    const first = mountBody("<p>one</p>");
    const second = mountBody("<p>two</p>");
    selectText(first.root.firstChild!.firstChild!, 1);
    first.text.value = "<p>one next</p>";
    await nextTick();
    expect(first.root.innerHTML).toBe("<p>one</p>");

    selectText(second.root.firstChild!.firstChild!, 1);
    selectionChanged();
    await nextTick();
    expect(first.root.innerHTML).toBe("<p>one next</p>");
  });

});
