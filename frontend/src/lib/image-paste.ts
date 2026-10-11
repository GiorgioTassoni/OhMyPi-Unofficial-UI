/** Handle image paste without swallowing ordinary text paste. */
export async function pasteImages(
  event: Pick<ClipboardEvent, "clipboardData" | "preventDefault">,
  nativeImage: () => Promise<Blob | null>,
  attach: (image: Blob) => Promise<void>,
): Promise<void> {
  const images = Array.from(event.clipboardData?.items ?? [])
    .filter((item) => item.kind === "file" && item.type.startsWith("image/"))
    .flatMap((item) => {
      const file = item.getAsFile();
      return file === null ? [] : [file];
    });
  if (images.length === 0) {
    // GTK WebKit can emit an empty event for a bitmap clipboard. Only probe the
    // native clipboard when there is no textual paste to leave to the textarea.
    const data = event.clipboardData;
    if (["text/plain", "text/html", "text/uri-list"].some((type) => data?.getData(type))) return;
    const image = await nativeImage();
    if (image !== null) await attach(image);
    return;
  }
  event.preventDefault();
  for (const image of images) await attach(image);
}
