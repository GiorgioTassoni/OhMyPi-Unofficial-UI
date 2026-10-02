import { describeBytes, type Attachment } from "./attachments";

/** Keep selected files useful without turning one attachment into an enormous prompt. */
export const MAX_TEXT_FILE_BYTES = 128 * 1024;

/** Browser file pickers expose bytes and a name, not an absolute path the agent can open. */
export async function readTextAttachment(file: File, id: string): Promise<Attachment> {
  if (file.size > MAX_TEXT_FILE_BYTES) {
    throw new Error(
      `${file.name} is ${describeBytes(file.size)}; text attachments are limited to ${describeBytes(MAX_TEXT_FILE_BYTES)}. Drop the file to send its path instead.`,
    );
  }

  // A PDF can start with ASCII, but decoding its bytes would not make it readable text.
  const mime = file.type.toLowerCase();
  if (mime && !mime.startsWith("text/") && !/(json|xml|yaml|toml|javascript|octet-stream)/.test(mime)) {
    throw new Error(
      `${file.name} is not a plain-text file. Choose a UTF-8 text or code file, or drop it to send its path.`,
    );
  }

  let content: string;
  try {
    content = new TextDecoder("utf-8", { fatal: true }).decode(await file.arrayBuffer());
  } catch {
    throw new Error(`${file.name} is not valid UTF-8 text. Drop it to send its path instead.`);
  }
  if (content.includes("\0")) {
    throw new Error(`${file.name} contains binary data. Drop it to send its path instead.`);
  }

  return { id, kind: "text", name: file.name, content, bytes: file.size };
}
