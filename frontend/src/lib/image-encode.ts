/**
 * The one browser-dependent half of attaching an image (`docs/12` §5.1).
 *
 * Everything else about attachments is pure and asserted (`lib/attachments.ts`): the
 * model, the frame arithmetic, the ladder of steps to try. This module is the part that
 * cannot be — reading bytes, decoding them, drawing them to a canvas and asking the
 * webview to encode the result — kept tiny and dependency-free for exactly that reason.
 *
 * It exists because of one number. An inbound command is a single unchunked JSONL frame
 * capped by what `ready` advertises (1 MiB at v18.2.6) and base64 inflates bytes by a
 * third, so a 1568-pixel PNG screenshot — 1.5 to 3 MB on disk — cannot be sent as it
 * stands while the same frame as WebP or JPEG can. Passing the bytes through and hoping
 * would mean a refusal the user only meets after pressing send.
 */

import {
  type Attachment,
  type EncodeStep,
  ENCODE_STEPS,
  describeBytes,
  scaleFor,
  sniffMime,
} from "./attachments";

/** Exactly what the engine accepts, as it accepts it. */
const ACCEPTED_TYPES = new Set([
  "image/png",
  "image/jpeg",
  "image/gif",
  "image/webp",
]);

/** What went wrong, in words the composer can put on screen. */
export class AttachmentError extends Error {}

/**
 * Read one image the user handed over, ready to attach.
 *
 * An image that already fits is passed through **untouched**: a 40 KB PNG of a diagram
 * has nothing to gain from a lossy re-encode, and the engine applies its own
 * normalization per model anyway. Re-encoding is the answer to not fitting, not a
 * habit.
 */
export async function readImage(
  file: Blob,
  name: string,
  id: string,
  room: number,
): Promise<Attachment> {
  const buffer = new Uint8Array(await file.arrayBuffer());
  const mime = typeOf(file, buffer);

  if (mime === null) {
    throw new AttachmentError(
      `${name} is not an image this app can attach — png, jpeg, gif or webp`,
    );
  }

  if (buffer.byteLength <= room) {
    return {
      id,
      kind: "image",
      name,
      data: toBase64(buffer),
      mime,
      bytes: buffer.byteLength,
    };
  }

  const encoded = await shrink(file, room, name);
  return {
    id,
    kind: "image",
    name,
    data: toBase64(new Uint8Array(await encoded.arrayBuffer())),
    mime: encoded.type,
    bytes: encoded.size,
  };
}

/**
 * The image's real type: what the browser claims, checked against what it is.
 *
 * `File.type` is a guess the browser made from a name or a system database, and an
 * attachment labelled wrongly is one the engine refuses. The bytes are the fact — the
 * engine sniffs the same way round — so the claim is used only when it is one of the
 * four, and the magic bytes decide otherwise.
 */
function typeOf(file: Blob, buffer: Uint8Array): string | null {
  if (ACCEPTED_TYPES.has(file.type)) {
    return file.type;
  }

  return sniffMime(buffer.subarray(0, 16));
}

/**
 * Re-encode down the ladder until it fits, or explain why it cannot.
 *
 * Every rung is **measured**: the encoder decides the size, so the loop stops at the
 * first result that fits rather than trusting the table. That also makes the refusal
 * honest — it is raised after the smallest, most compressed attempt, not because the
 * first try failed.
 */
async function shrink(source: Blob, room: number, name: string): Promise<Blob> {
  let bitmap: ImageBitmap;
  try {
    bitmap = await createImageBitmap(source);
  } catch {
    // A file that carries image magic bytes and does not decode: truncated, or a
    // format the webview's decoder does not know.
    throw new AttachmentError(`${name} could not be decoded as an image`);
  }

  try {
    // The first rung is tried before the loop so the refusal has a measured size to
    // report without encoding anything twice.
    let smallest = await encode(bitmap, ENCODE_STEPS[0]);
    if (smallest.size <= room) {
      return smallest;
    }

    for (const step of ENCODE_STEPS.slice(1)) {
      const candidate = await encode(bitmap, step);
      if (candidate.size <= room) {
        return candidate;
      }
      if (candidate.size < smallest.size) {
        smallest = candidate;
      }
    }

    throw new AttachmentError(
      `${name} is too large to send: re-encoded down to ${describeBytes(smallest.size)} it still does not fit the ${describeBytes(room)} this window can carry per image`,
    );
  } finally {
    bitmap.close();
  }
}

/** Draw the bitmap at one rung of the ladder and encode it. */
async function encode(bitmap: ImageBitmap, step: EncodeStep): Promise<Blob> {
  const scale = scaleFor(bitmap.width, bitmap.height, step.maxEdge);
  const canvas = document.createElement("canvas");
  // `max(1, …)`: a canvas of zero width throws, and a 1-pixel floor is the right answer
  // for an image scaled to nothing.
  canvas.width = Math.max(1, Math.round(bitmap.width * scale));
  canvas.height = Math.max(1, Math.round(bitmap.height * scale));

  const context = canvas.getContext("2d");
  if (context === null) {
    throw new AttachmentError("this window cannot draw images to resize them");
  }
  context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);

  return toBlob(canvas, context, step.quality);
}

/**
 * Encode a canvas, reading back what actually came out.
 *
 * `toBlob` **substitutes silently**: asked for a type the platform's encoder does not
 * provide, WebKit and Chromium answer with `image/png` rather than failing. Whether
 * WebP encoding exists is a property of the installed WebKit, not of anything this app
 * controls — so the result's own `type` is what decides, and a PNG answer is not trusted
 * to be a WebP one. (PNG is never the target of a *re-encode* anyway: not being able to
 * shrink is why the image is here.)
 *
 * The fallback is JPEG, except where JPEG would destroy the picture: an image with
 * transparency keeps PNG, because flattening a screenshot's alpha onto black is a
 * change to what the model sees.
 */
async function toBlob(
  canvas: HTMLCanvasElement,
  context: CanvasRenderingContext2D,
  quality: number,
): Promise<Blob> {
  const webp = await encodeAs(canvas, "image/webp", quality);
  if (webp !== null) {
    return webp;
  }

  if (hasTransparency(context, canvas.width, canvas.height)) {
    const png = await encodeAs(canvas, "image/png");
    if (png !== null) {
      return png;
    }
  }

  const jpeg = await encodeAs(canvas, "image/jpeg", quality);
  if (jpeg !== null) {
    return jpeg;
  }

  throw new AttachmentError("this window cannot re-encode images");
}

/** `toBlob` as a promise, or `null` when the platform answered with another type. */
async function encodeAs(
  canvas: HTMLCanvasElement,
  type: string,
  quality?: number,
): Promise<Blob | null> {
  const blob = await new Promise<Blob | null>((resolve) => {
    canvas.toBlob(resolve, type, quality);
  });

  return blob !== null && blob.type === type ? blob : null;
}

/**
 * Whether any pixel is not fully opaque.
 *
 * Scanned rather than sampled: a screenshot's transparency is usually a rounded corner
 * or a drop shadow, and a sampled grid would miss exactly those. Only reached when WebP
 * is unavailable, so the cost is on a path most platforms never take.
 */
function hasTransparency(
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
): boolean {
  const pixels = context.getImageData(0, 0, width, height).data;
  for (let at = 3; at < pixels.length; at += 4) {
    if (pixels[at] !== 0xff) {
      return true;
    }
  }

  return false;
}

/**
 * Base64, chunked.
 *
 * `String.fromCharCode(...bytes)` throws once the array is longer than the engine's
 * argument limit — around a hundred thousand entries — and an attachment is orders of
 * magnitude past that. Chunking is not a micro-optimization here; it is the difference
 * between working and not.
 */
function toBase64(bytes: Uint8Array): string {
  const CHUNK = 0x8000;
  let binary = "";

  for (let at = 0; at < bytes.length; at += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(at, at + CHUNK));
  }

  return btoa(binary);
}
