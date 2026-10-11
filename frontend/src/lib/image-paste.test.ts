import { expect, test } from "bun:test";
import { pasteImages } from "./image-paste";
import { readImage } from "./image-encode";
import type { Attachment } from "./attachments";

const png = new File([new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10])], "image.png", { type: "image/png" });

function paste(items: DataTransferItem[] = [], text = "") {
  let prevented = false;
  return {
    event: {
      clipboardData: { items, getData: () => text } as unknown as DataTransfer,
      preventDefault() { prevented = true; },
    },
    prevented: () => prevented,
  };
}

test("an empty WebKit paste event still produces an image attachment from the native clipboard", async () => {
  const input = paste();
  const attached: Attachment[] = [];
  await pasteImages(input.event, async () => png, async (blob) => {
    attached.push(await readImage(blob, "pasted image", "id", 1024));
  });
  expect(attached).toEqual([{
    id: "id", kind: "image", name: "pasted image", mime: "image/png", bytes: 8,
    data: "iVBORw0KGgo=",
  }]);
});

test("ordinary text paste stays native even if the OS also offers an image", async () => {
  const input = paste([], "hello");
  const attached: Blob[] = [];
  await pasteImages(input.event, async () => png, async (blob) => { attached.push(blob); });
  expect(input.prevented()).toBe(false);
  expect(attached).toEqual([]);
});

test("DOM image paste attaches only once without consulting the native clipboard", async () => {
  const item = { kind: "file", type: "image/png", getAsFile: () => png } as DataTransferItem;
  const input = paste([item]);
  const attached: Blob[] = [];
  await pasteImages(input.event, async () => { throw new Error("must not read twice"); }, async (blob) => { attached.push(blob); });
  expect(input.prevented()).toBe(true);
  expect(attached).toEqual([png]);
});

test("an empty clipboard does not add an attachment", async () => {
  const input = paste();
  const attached: Blob[] = [];
  await pasteImages(input.event, async () => null, async (blob) => { attached.push(blob); });
  expect(attached).toEqual([]);
});
