export interface TextSegment {
  text: string;
  isColor: boolean;
}

export function isHexColor(raw: string): boolean {
  if (!raw || !raw.startsWith("#")) return false;
  const hex = raw.slice(1);
  if (hex.length === 6 || hex.length === 8) {
    return /^[0-9a-fA-F]+$/.test(hex);
  }
  if (hex.length === 3) {
    if (!/^[0-9a-fA-F]{3}$/.test(hex)) return false;
    if (/^\d{3}$/.test(hex)) {
      return hex[0] === hex[1] && hex[1] === hex[2];
    }
    return true;
  }
  return false;
}

export function parseTextSegments(text: string): TextSegment[] {
  if (!text || !text.includes("#")) {
    return [{ text: text ?? "", isColor: false }];
  }
  const regex = /#([0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{3})\b/g;
  const segments: TextSegment[] = [];
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = regex.exec(text)) !== null) {
    const raw = match[0];
    if (isHexColor(raw)) {
      if (match.index > lastIndex) {
        segments.push({ text: text.slice(lastIndex, match.index), isColor: false });
      }
      segments.push({ text: raw, isColor: true });
      lastIndex = regex.lastIndex;
    }
  }

  if (lastIndex < text.length) {
    segments.push({ text: text.slice(lastIndex), isColor: false });
  }

  return segments.length > 0 ? segments : [{ text, isColor: false }];
}
