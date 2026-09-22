/**
 * The attachment rules, asserted (`docs/12` §5.1).
 *
 * The cases are the ones that would otherwise reach a provider as a bill: bytes that
 * became prompt text, a frame the transport refuses after the user pressed send, an
 * image silently re-encoded when it already fit, an encoder that upscaled a small
 * picture. Every one of them is invisible in review and one line in a component.
 */

import { describe, expect, test } from "bun:test";
import {
  type Attachment,
  type ImageAttachment,
  base64Length,
  describeBytes,
  encodedSize,
  estimateOverhead,
  fitsFrame,
  frameRefusal,
  imageBudget,
  imagesToSend,
  messageFor,
  nameFromPath,
  roomForAnotherImage,
  scaleFor,
  sniffMime,
  tooLargeMessage,
} from "./attachments";

/** The frame the engine advertised on this workstation (`ready.maxFrameBytes`). */
const FRAME_LIMIT = 1_048_576;

function image(bytes: number, name = "shot.png"): ImageAttachment {
  return {
    id: name,
    kind: "image",
    name,
    data: "A".repeat(base64Length(bytes)),
    mime: "image/png",
    bytes,
  };
}

function file(path: string): Attachment {
  return { id: path, kind: "path", name: nameFromPath(path), path };
}

describe("the frame budget", () => {
  test("base64 costs a third again, rounded up to four", () => {
    expect(base64Length(0)).toBe(0);
    expect(base64Length(1)).toBe(4);
    expect(base64Length(3)).toBe(4);
    expect(base64Length(4)).toBe(8);
    expect(base64Length(1024)).toBe(1368);
  });

  test("the overhead estimate is an upper bound on the real frame", () => {
    // The estimate uses a longer id than the transport assigns, so it cannot be
    // under the real frame. An estimate that was *low* would let a send through that
    // the client refuses afterwards, which is the failure the refusal exists for.
    const message = "why is this failing?";
    const real = JSON.stringify({
      id: "req_7",
      type: "prompt",
      message,
      images: [{ type: "image", data: "AAAA", mimeType: "image/png" }],
    }).length;

    expect(estimateOverhead(message, 1)).toBeGreaterThan(real);
  });

  test("the budget is the largest image that fits, and the next block does not", () => {
    const message = "what is this?";
    const budget = roomForAnotherImage(FRAME_LIMIT, message, []);

    expect(fitsFrame(FRAME_LIMIT, message, [image(budget)])).toBe(true);
    // Base64 grows in four-byte steps, so "the largest that fits" is only defined to
    // within a block — and a budget that left a whole block unused would refuse an
    // image that would have been fine.
    expect(fitsFrame(FRAME_LIMIT, message, [image(budget + 3)])).toBe(false);
  });

  test("a second image is budgeted against the first", () => {
    const message = "compare";
    const first = image(600_000, "a.png");
    const alone = roomForAnotherImage(FRAME_LIMIT, message, []);
    const afterOne = roomForAnotherImage(FRAME_LIMIT, message, [first]);

    expect(afterOne).toBeLessThan(alone);
    // Two images at the single-image budget are exactly the case that would fail late.
    expect(fitsFrame(FRAME_LIMIT, message, [image(alone), image(alone)])).toBe(false);
  });

  test("paths cost the frame nothing", () => {
    const message = "read this";
    const attached = [file("/tmp/notes.md")];

    expect(encodedSize(attached)).toBe(0);
    expect(fitsFrame(FRAME_LIMIT, message, attached)).toBe(true);
  });

  test("a screenshot that fits the disk does not fit the wire", () => {
    // The number that shapes the composer: a 1568px PNG screenshot is 1.5–3 MB, and
    // 1 MiB of frame carries about 780 KB of raw image.
    expect(imageBudget(FRAME_LIMIT)).toBeGreaterThan(700 * 1024);
    expect(imageBudget(FRAME_LIMIT)).toBeLessThan(800 * 1024);
    expect(fitsFrame(FRAME_LIMIT, "look", [image(2_000_000)])).toBe(false);
  });

  test("a budget of nothing is zero, not negative", () => {
    expect(imageBudget(0)).toBe(0);
    expect(imageBudget(-1)).toBe(0);
    // A message long enough to eat the whole frame leaves no room for an image —
    // and the answer must be a number a caller can compare, not a negative one.
    expect(roomForAnotherImage(FRAME_LIMIT, "x".repeat(FRAME_LIMIT), [])).toBe(0);
  });
});

describe("the two routes", () => {
  test("images never become prompt text", () => {
    // The rule the whole design turns on. Base64 in `message` is text tokens, and one
    // screenshot is hundreds of thousands of them.
    const message = "what is this?";

    expect(messageFor(message, [image(200_000)])).toBe(message);
    const [wire] = imagesToSend([image(200_000)]);
    expect(wire.data.length).toBe(base64Length(200_000));
    expect(message.includes(wire.data)).toBe(false);
  });

  test("paths are appended after the draft, one per line", () => {
    const sent = messageFor("  summarise these  ", [
      file("/tmp/a.md"),
      file("/tmp/b with spaces.pdf"),
    ]);

    expect(sent).toBe("summarise these\n\n/tmp/a.md\n/tmp/b with spaces.pdf");
  });

  test("a path with no draft is the whole message", () => {
    expect(messageFor("   ", [file("/tmp/a.md")])).toBe("/tmp/a.md");
    expect(messageFor("", [])).toBe("");
  });

  test("images keep the order they were attached in", () => {
    const first = { ...image(10, "first.png"), data: "FIRST" };
    const second = { ...image(10, "second.webp"), data: "SECOND", mime: "image/webp" };

    expect(imagesToSend([first, file("/tmp/a.md"), second])).toEqual([
      { data: "FIRST", mimeType: "image/png" },
      { data: "SECOND", mimeType: "image/webp" },
    ]);
  });
});

describe("what the user sees", () => {
  test("a size label reads the way a person would say it", () => {
    expect(describeBytes(812)).toBe("812 B");
    expect(describeBytes(2048)).toBe("2 KB");
    expect(describeBytes(1_500_000)).toBe("1.4 MB");
  });

  test("a refusal names both numbers", () => {
    const refusal = tooLargeMessage(image(2_000_000, "shot.png"), 786_000);

    expect(refusal).toContain("shot.png");
    expect(refusal).toContain("1.9 MB");
    expect(refusal).toContain("768 KB");
  });

  test("a message that will not fit says so before it is sent", () => {
    const message = "look";
    expect(frameRefusal(FRAME_LIMIT, message, [image(200_000)])).toBeNull();

    const refusal = frameRefusal(FRAME_LIMIT, message, [image(2_000_000, "shot.png")]);
    expect(refusal).toContain("shot.png");
    expect(refusal).toContain("1.9 MB");
    expect(refusal).toContain("1.0 MB");
  });

  test("a draft can outgrow the frame with nothing attached", () => {
    // Not a hypothetical: a pasted log is megabytes of text, and the transport would
    // refuse it after the user pressed send.
    const refusal = frameRefusal(FRAME_LIMIT, "x".repeat(FRAME_LIMIT), []);

    expect(refusal).toContain("the message text alone is over it");
    expect(frameRefusal(FRAME_LIMIT, "hello", [])).toBeNull();
  });

  test("the type comes from the bytes, not from the browser's claim", () => {
    const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
    const jpeg = new Uint8Array([0xff, 0xd8, 0xff, 0xe0]);
    const gif = new Uint8Array([0x47, 0x49, 0x46, 0x38, 0x39, 0x61]);
    const webp = new Uint8Array([
      0x52, 0x49, 0x46, 0x46, 0x00, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50,
    ]);

    expect(sniffMime(png)).toBe("image/png");
    expect(sniffMime(jpeg)).toBe("image/jpeg");
    expect(sniffMime(gif)).toBe("image/gif");
    expect(sniffMime(webp)).toBe("image/webp");
    // A PDF or a text file is not an image at all, and saying so is the answer.
    expect(sniffMime(new Uint8Array([0x25, 0x50, 0x44, 0x46]))).toBeNull();
    expect(sniffMime(new Uint8Array([]))).toBeNull();
  });

  test("a small image is never upscaled", () => {
    // `scaleFor` returning >1 for a 100×100 icon would turn a 3 KB file into a
    // megabyte of interpolated nothing.
    expect(scaleFor(100, 80, 1568)).toBe(1);
    expect(scaleFor(0, 0, 1568)).toBe(1);
    expect(scaleFor(3136, 1000, 1568)).toBe(0.5);
    expect(scaleFor(1000, 3136, 1568)).toBe(0.5);
  });

  test("a name comes off the path it arrived on", () => {
    expect(nameFromPath("/tmp/shots/screen 1.png")).toBe("screen 1.png");
    expect(nameFromPath("/tmp/shots/")).toBe("shots");
    expect(nameFromPath("C:\\Users\\gio\\shot.png")).toBe("shot.png");
  });
});
