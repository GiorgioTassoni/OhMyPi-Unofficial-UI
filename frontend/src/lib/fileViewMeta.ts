/**
 * Pure parsing, extension normalization, and metadata helpers for the file viewer.
 */

export const IMAGE_EXTENSIONS = new Set([
  "png",
  "jpg",
  "jpeg",
  "gif",
  "webp",
  "svg",
  "bmp",
  "ico",
  "avif",
  "tiff",
  "tif",
]);

export const VIDEO_EXTENSIONS = new Set([
  "webm",
  "mp4",
  "mov",
  "mkv",
  "ogg",
  "avi",
]);

const MARKDOWN_EXTENSIONS = new Set(["md", "markdown", "mdown", "mkd"]);

export interface FilePathMeta {
  cleanPath: string;
  lineRange: string | null;
  basename: string;
  directory: string | null;
  extension: string;
}

/**
 * Normalizes an extension or path tail by stripping line ranges (`:239-245`) or anchor fragments.
 */
export function cleanExtension(ext: string): string {
  if (!ext) return "";
  const stripped = ext.toLowerCase().trim().replace(/(?:\?.*|#.*|:\d+(?:-\d+)?)$/, "");
  const lastDot = stripped.lastIndexOf(".");
  return lastDot >= 0 ? stripped.slice(lastDot + 1) : stripped;
}

export function normalizeFilePath(rawPath: string): string {
  let trimmed = rawPath.trim();
  if (/^file:\/\/\//i.test(trimmed)) {
    trimmed = trimmed.slice(7);
  } else if (/^file:\/\//i.test(trimmed)) {
    const after = trimmed.slice(7);
    trimmed = after.startsWith("/") || /^[a-zA-Z]:/.test(after) ? after : "/" + after;
  } else if (/^file:/i.test(trimmed)) {
    trimmed = trimmed.slice(5);
  }
  if (trimmed.includes("%")) {
    try {
      trimmed = decodeURIComponent(trimmed);
    } catch {}
  }
  return trimmed;
}

/**
 * Parses raw paths (which may include line range suffixes from tool outputs like `src/main.dart:239-245`).
 */
export function parseFilePath(rawPath: string): FilePathMeta {
  if (!rawPath) {
    return { cleanPath: "", lineRange: null, basename: "", directory: null, extension: "" };
  }

  const normalized = normalizeFilePath(rawPath);

  // Strip line number or hash ranges (e.g. `:239-245` or `#L10-L20`)
  const lineMatch = normalized.match(/(?::(\d+(?:-\d+)?)|#L?(\d+(?:-L?\d+)?))$/);
  const rawRange = lineMatch ? (lineMatch[1] || lineMatch[2]) : null;
  const lineRange = rawRange ? rawRange.replace(/L/g, "") : null;
  const cleanPath = lineMatch ? normalized.slice(0, lineMatch.index) : normalized;

  // Handle skill URIs (e.g. "skill://impeccable", "skills://impeccable", "skill:impeccable")
  if (/^skills?:/i.test(cleanPath)) {
    const schemeMatch = cleanPath.match(/^skills?:(?:\/\/)?/i);
    const prefix = schemeMatch ? schemeMatch[0] : "skill://";
    const skillName = cleanPath.slice(prefix.length).replace(/\/.*$/, "");
    return {
      cleanPath,
      lineRange,
      basename: skillName || cleanPath,
      directory: prefix.includes("//") ? "skill://" : "skill:",
      extension: "md",
    };
  }

  const pathWithoutQuery = cleanPath.replace(/\?.*$/, "");
  const parts = pathWithoutQuery.split(/[/\\]/).filter(Boolean);
  const basename = parts.pop() || cleanPath;

  let directory: string | null = null;
  if (/^https?:\/\//i.test(cleanPath)) {
    try {
      const u = new URL(cleanPath);
      const lastSlash = u.pathname.lastIndexOf("/");
      directory = `${u.origin}${lastSlash > 0 ? u.pathname.slice(0, lastSlash) : ""}`;
    } catch {
      directory = null;
    }
  } else if (cleanPath.endsWith(basename)) {
    const dir = cleanPath.slice(0, cleanPath.length - basename.length).replace(/[/\\]$/, "");
    directory = dir || null;
  }

  const dot = basename.lastIndexOf(".");
  const extension = dot > 0 ? basename.slice(dot + 1).toLowerCase() : "";

  return { cleanPath, lineRange, basename, directory, extension };
}

export function isImageExtension(ext: string): boolean {
  return IMAGE_EXTENSIONS.has(cleanExtension(ext));
}

export function isVideoExtension(ext: string): boolean {
  return VIDEO_EXTENSIONS.has(cleanExtension(ext));
}

export function isSvgExtension(ext: string): boolean {
  return cleanExtension(ext) === "svg";
}

export function isMarkdownExtension(ext: string): boolean {
  return MARKDOWN_EXTENSIONS.has(cleanExtension(ext));
}

export function mimeTypeForExtension(ext: string): string {
  switch (cleanExtension(ext)) {
    case "svg": return "image/svg+xml";
    case "png": return "image/png";
    case "jpg":
    case "jpeg": return "image/jpeg";
    case "gif": return "image/gif";
    case "webp": return "image/webp";
    case "bmp": return "image/bmp";
    case "ico": return "image/x-icon";
    case "avif": return "image/avif";
    case "webm": return "video/webm";
    case "mp4": return "video/mp4";
    case "mov": return "video/quicktime";
    case "ogg": return "video/ogg";
    case "mkv": return "video/x-matroska";
    default: return "image/png";
  }
}

const LANGUAGE_LABELS: Record<string, string> = {
  ts: "TYPESCRIPT",
  tsx: "TSX",
  js: "JAVASCRIPT",
  jsx: "JSX",
  vue: "VUE",
  dart: "DART",
  rs: "RUST",
  py: "PYTHON",
  json: "JSON",
  yml: "YAML",
  yaml: "YAML",
  toml: "TOML",
  md: "MARKDOWN",
  css: "CSS",
  scss: "SCSS",
  html: "HTML",
  sh: "SHELL",
  bash: "BASH",
  sql: "SQL",
  go: "GO",
  c: "C",
  cpp: "C++",
  h: "C HEADER",
  png: "PNG",
  jpg: "JPEG",
  jpeg: "JPEG",
  gif: "GIF",
  webp: "WEBP",
  svg: "SVG",
  bmp: "BMP",
  ico: "ICO",
  avif: "AVIF",
};

export function formatLanguageLabel(ext: string): string {
  const clean = cleanExtension(ext);
  if (!clean) return "TEXT";
  return LANGUAGE_LABELS[clean] ?? clean.toUpperCase();
}

export function stripToolOutputFormatting(text: string): string {
  if (!text) return "";
  let clean = text;
  const hadHeader = /^\[[^\n\r]+(?:#[a-fA-F0-9]+)?\]\r?\n?/.test(clean);
  clean = clean.replace(/^\[[^\n\r]+#[a-fA-F0-9]+\]\r?\n?/, "");
  clean = clean.replace(/^\[[^\n\r]+\]\r?\n?/, "");

  const lines = clean.split(/\r?\n/);
  const nonEmptyLines = lines.filter((l) => l.trim().length > 0);
  const numberedLines = lines.filter((l) => /^\s*\d+\s*[:|]\s?/.test(l));

  if (hadHeader && numberedLines.length > 0) {
    return lines.map((l) => l.replace(/^\s*\d+\s*[:|]\s?/, "")).join("\n");
  }

  if (
    nonEmptyLines.length >= 2 &&
    numberedLines.length >= 2 &&
    numberedLines.length / nonEmptyLines.length > 0.6
  ) {
    return lines.map((l) => l.replace(/^\s*\d+\s*[:|]\s?/, "")).join("\n");
  }

  return clean;
}
