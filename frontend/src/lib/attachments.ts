/**
 * What the composer attaches and how it fits (`docs/12` §5.1, "three routes, one rule").
 *
 * The rule: **an attachment never becomes prompt text.** Bytes the browser holds go to
 * the engine as `prompt{images}` — base64 in `message` would be text tokens, and one
 * screenshot is hundreds of thousands of them. Anything with a path goes in as its path,
 * because the agent can read a file and the engine's own pipeline (conversion, size
 * caps, resize, `?q=` vision questions) then applies to it.
 *
 * The size budget shapes the UI, so the arithmetic lives here rather than in a component:
 * an inbound command is one unchunked JSONL frame, the engine advertises the limit in
 * `ready` (1 MiB at v18.2.6), and base64 inflates bytes by a third. A screenshot that
 * fits the disk does not fit the wire, which is why [`ENCODE_STEPS`] exists and why a
 * refusal has to be visible *before* a send rather than discovered by one.
 *
 * Everything in this module is pure. The canvas that re-encodes is
 * `lib/image-encode.ts`, so these rules are asserted without a browser.
 */

import type { ImageIn } from "../bridge";

/** One attachment the composer is holding, before it is sent. */
export type Attachment =
  | {
      /** Stable for the composer's lifetime; the strip keys on it. */
      id: string;
      kind: "image";
      /** What to call it: the file's name, or `pasted image`. */
      name: string;
      /** Base64 bytes — the wire's `data`, and what the thumbnail renders. */
      data: string;
      /** `image/png`, `image/jpeg`, `image/gif` or `image/webp`. */
      mime: string;
      /** Decoded size, for the size label and the budget. */
      bytes: number;
    }
  | {
      id: string;
      kind: "path";
      /** The file's own name, for the chip's label. */
      name: string;
      /**
       * Absolute path, as the OS reported it.
       *
       * No size: a drop tells the window where the file is and nothing more, and
       * reading it here would mean asking for a filesystem permission this app
       * deliberately does not hold. The engine reports what it read when the agent
       * opens it, which is the same fact from the side that matters.
       */
      path: string;
    };

/** The bytes route: the variant that goes to the engine as `prompt{images}`. */
export type ImageAttachment = Extract<Attachment, { kind: "image" }>;

/** How many bytes of base64 a payload of `bytes` becomes. */
export function base64Length(bytes: number): number {
  return 4 * Math.ceil(bytes / 3);
}

/**
 * The room a message's non-image fields take in the frame.
 *
 * Deliberately an upper bound rather than an exact figure: the host assigns the request
 * id (the transport counts in `req_N`, a few bytes) and this cannot see it, so the
 * estimate uses a comfortably longer id and lets the transport be the exact backstop.
 * Erring low here would put a send on the wire that the client refuses *after* the
 * user pressed send — which is the case the visible refusal exists to prevent.
 */
export function estimateOverhead(message: string, imageCount: number): number {
  const envelope = {
    id: "req_0000000000",
    type: "prompt",
    message,
    // One placeholder per image: the keys and the mime are real, the base64 is not,
    // because the payload is what is being budgeted for.
    images: Array.from({ length: imageCount }, () => ({
      type: "image",
      data: "",
      mimeType: "image/webp",
    })),
  };

  // `+ 1` for the newline that ends the JSONL frame.
  return JSON.stringify(envelope).length + 1;
}

/** What every attachment in a message costs the frame, as base64. */
export function encodedSize(attachments: readonly Attachment[]): number {
  return attachments.reduce(
    (total, attachment) =>
      total + (attachment.kind === "image" ? base64Length(attachment.bytes) : 0),
    0,
  );
}

/**
 * The largest image, in raw bytes, whose base64 still fits `frameLimit`.
 *
 * `4 * ceil(n / 3) <= 4 * (n + 2) / 3`, so requiring the right-hand side to fit and
 * then stepping back two bytes is a bound rather than an approximation — the caller
 * never sends a frame the client would refuse for being a few bytes over.
 */
export function imageBudget(frameLimit: number): number {
  const room = frameLimit;
  if (room <= 0) {
    return 0;
  }

  return Math.max(0, Math.floor((room * 3) / 4) - 2);
}

/**
 * The room left for one more image, given the message and what is already attached.
 *
 * The question the composer actually asks when something arrives — at attach time it
 * normalizes the image towards this number, and a second image is budgeted against the
 * first rather than against the frame on its own.
 */
export function roomForAnotherImage(
  frameLimit: number,
  message: string,
  attachments: readonly Attachment[],
): number {
  const claimed =
    encodedSize(attachments) + estimateOverhead(message, attachments.length + 1);

  return imageBudget(frameLimit - claimed);
}

/**
 * Why this message cannot go as it stands, or `null` when it fits.
 *
 * Checked before a send as well as at attach time, because the two are different
 * failures: an attachment can be normalized on the way in, but a draft can grow after
 * it (a pasted log, six paragraphs more) and push the frame over with nothing to
 * normalize. Saying which attachment is the largest is the part that makes it
 * actionable.
 */
export function frameRefusal(
  frameLimit: number,
  message: string,
  attachments: readonly Attachment[],
): string | null {
  if (fitsFrame(frameLimit, message, attachments)) {
    return null;
  }

  const total =
    encodedSize(attachments) + estimateOverhead(message, attachments.length);
  const largest = attachments
    .filter((attachment): attachment is ImageAttachment => attachment.kind === "image")
    .reduce<ImageAttachment | null>(
      (biggest, attachment) =>
        biggest === null || attachment.bytes > biggest.bytes ? attachment : biggest,
      null,
    );
  const who =
    largest === null
      ? " — the message text alone is over it"
      : `, and the largest image is ${largest.name} at ${describeBytes(largest.bytes)}`;

  return `this message needs ${describeBytes(total)} of the ${describeBytes(frameLimit)} frame${who}`;
}

/** Whether the message and everything attached to it fit one frame. */
export function fitsFrame(
  frameLimit: number,
  message: string,
  attachments: readonly Attachment[],
): boolean {
  return (
    encodedSize(attachments) + estimateOverhead(message, attachments.length) <=
    frameLimit
  );
}

/**
 * The images to send, in the order they were attached.
 *
 * Order is kept because the engine numbers a message's images `[Image #N]` in this
 * order — reordering here would attach a picture to the wrong sentence.
 */
export function imagesToSend(attachments: readonly Attachment[]): ImageIn[] {
  return attachments
    .filter((attachment): attachment is ImageAttachment => attachment.kind === "image")
    .map((attachment) => ({ data: attachment.data, mimeType: attachment.mime }));
}

/**
 * The message text that goes on the wire: the draft, then one line per attached path.
 *
 * Paths are appended here rather than shown in the box, so the strip stays the single
 * account of what is attached: removing a chip removes the path. A path the user typed
 * themselves is untouched — this only adds what the composer is holding.
 */
export function messageFor(
  draft: string,
  attachments: readonly Attachment[],
): string {
  const paths = attachments
    .filter((attachment) => attachment.kind === "path")
    .map((attachment) => attachment.path);

  if (paths.length === 0) {
    return draft.trim();
  }

  const text = draft.trim();

  return text === "" ? paths.join("\n") : `${text}\n\n${paths.join("\n")}`;
}

/** `184 KB`, `1.4 MB`, `812 B` — the size label on a chip. */
export function describeBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${Math.round(bytes / 1024)} KB`;
  }

  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** Why an image cannot be sent, in the two numbers the user needs. */
export function tooLargeMessage(
  attachment: ImageAttachment,
  budget: number,
): string {
  return `${attachment.name} is ${describeBytes(attachment.bytes)}; this window can carry ${describeBytes(budget)} per image`;
}

/**
 * The magic bytes the engine itself reads, for the four types it accepts.
 *
 * The browser's `File.type` is a claim — a screenshot pasted on Linux often arrives as
 * `image/png` (correct) but a file chosen from disk can carry an empty type or
 * `application/octet-stream`. The bytes are the fact, and the engine sniffs the same
 * way, so a wrong claim here would be an attachment the engine refuses.
 */
export function sniffMime(head: Uint8Array): string | null {
  const starts = (...magic: number[]): boolean =>
    magic.every((byte, at) => head[at] === byte);

  if (starts(0x89, 0x50, 0x4e, 0x47)) {
    return "image/png";
  }
  if (starts(0xff, 0xd8, 0xff)) {
    return "image/jpeg";
  }
  if (starts(0x47, 0x49, 0x46, 0x38)) {
    return "image/gif";
  }
  // `RIFF....WEBP`
  if (starts(0x52, 0x49, 0x46, 0x46) && head[8] === 0x57 && head[9] === 0x45) {
    return "image/webp";
  }

  return null;
}

/** One rung of the re-encode ladder. */
export interface EncodeStep {
  /** Longest edge, in pixels, the image is scaled to fit. */
  maxEdge: number;
  /** Encoder quality, 0–1. */
  quality: number;
}

/**
 * What to try when an image does not fit, cheapest first.
 *
 * Quality before pixels: a screenshot at 1568px and 0.85 is around 150–400 KB, so the
 * first rung usually lands and the ones after it are for photographs and dense UI.
 * 1568px is the engine's own target (`images.autoResize` resizes to 2000×2000, and its
 * internal pipeline works to 1568 on the longest edge), so a resize here is not losing
 * detail the model would have seen.
 *
 * Every step is *measured* when it runs — the encoder decides the actual size, so the
 * loop stops at the first rung that fits rather than trusting this table.
 */
export const ENCODE_STEPS: readonly EncodeStep[] = [
  { maxEdge: 1568, quality: 0.85 },
  { maxEdge: 1568, quality: 0.7 },
  { maxEdge: 1280, quality: 0.7 },
  { maxEdge: 1024, quality: 0.6 },
  { maxEdge: 768, quality: 0.5 },
  { maxEdge: 512, quality: 0.5 },
];

/** The scale factor that fits `width`×`height` inside `maxEdge`. */
export function scaleFor(
  width: number,
  height: number,
  maxEdge: number,
): number {
  const longest = Math.max(width, height);
  if (longest <= maxEdge || longest === 0) {
    return 1;
  }

  return maxEdge / longest;
}

/** The name to call a file, given the path the OS reported. */
export function nameFromPath(path: string): string {
  const parts = path.split(/[/\\]/).filter((part) => part !== "");

  return parts[parts.length - 1] ?? path;
}
