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
  const stripped = ext.toLowerCase().trim().replace(/(?::\d+(?:-\d+)?|#.*)$/, "");
  const lastDot = stripped.lastIndexOf(".");
  return lastDot >= 0 ? stripped.slice(lastDot + 1) : stripped;
}

/**
 * Parses raw paths (which may include line range suffixes from tool outputs like `src/main.dart:239-245`).
 */
export function parseFilePath(rawPath: string): FilePathMeta {
  if (!rawPath) {
    return { cleanPath: "", lineRange: null, basename: "", directory: null, extension: "" };
  }

  // Strip line number or hash ranges (e.g. `:239-245` or `#L10-L20`)
  const lineMatch = rawPath.match(/(?::(\d+(?:-\d+)?)|#L?(\d+(?:-L?\d+)?))$/);
  const rawRange = lineMatch ? (lineMatch[1] || lineMatch[2]) : null;
  const lineRange = rawRange ? rawRange.replace(/L/g, "") : null;
  const cleanPath = lineMatch ? rawPath.slice(0, lineMatch.index) : rawPath;

  const parts = cleanPath.split(/[/\\]/).filter(Boolean);
  const basename = parts.pop() || cleanPath;

  let directory: string | null = null;
  if (cleanPath.endsWith(basename)) {
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
