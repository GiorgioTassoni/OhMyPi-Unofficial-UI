import { describe, expect, test } from "bun:test";
import {
  cleanExtension,
  formatLanguageLabel,
  isImageExtension,
  isMarkdownExtension,
  isSvgExtension,
  isVideoExtension,
  mimeTypeForExtension,
  normalizeFilePath,
  parseFilePath,
} from "./fileViewMeta";

describe("fileViewMeta", () => {
  test("parseFilePath extracts cleanPath, lineRange, basename, and directory", () => {
    const meta = parseFilePath("/home/user/project/src/main.dart:239-245");
    expect(meta.cleanPath).toBe("/home/user/project/src/main.dart");
    expect(meta.lineRange).toBe("239-245");
    expect(meta.basename).toBe("main.dart");
    expect(meta.directory).toBe("/home/user/project/src");
    expect(meta.extension).toBe("dart");
  });

  test("parseFilePath handles hash line ranges", () => {
    const meta = parseFilePath("components/App.vue#L10-L20");
    expect(meta.cleanPath).toBe("components/App.vue");
    expect(meta.lineRange).toBe("10-20");
    expect(meta.basename).toBe("App.vue");
    expect(meta.directory).toBe("components");
    expect(meta.extension).toBe("vue");
  });

  test("parseFilePath handles paths without line ranges or directory", () => {
    const meta = parseFilePath("README.md");
    expect(meta.cleanPath).toBe("README.md");
    expect(meta.lineRange).toBeNull();
    expect(meta.basename).toBe("README.md");
    expect(meta.directory).toBeNull();
    expect(meta.extension).toBe("md");
  });

  test("parseFilePath recognizes skill URIs as markdown documents", () => {
    const meta = parseFilePath("skill://impeccable");
    expect(meta.cleanPath).toBe("skill://impeccable");
    expect(meta.basename).toBe("impeccable");
    expect(meta.directory).toBe("skill://");
    expect(meta.extension).toBe("md");

    const metaWithRange = parseFilePath("skill://impeccable:10-25");
    expect(metaWithRange.cleanPath).toBe("skill://impeccable");
    expect(metaWithRange.lineRange).toBe("10-25");
    expect(metaWithRange.basename).toBe("impeccable");
    expect(metaWithRange.directory).toBe("skill://");
    expect(metaWithRange.extension).toBe("md");
  });

  test("cleanExtension normalizes suffixes and leading dots", () => {
    expect(cleanExtension("dart:239-245")).toBe("dart");
    expect(cleanExtension("file.ts#L5")).toBe("ts");
    expect(cleanExtension(".rs")).toBe("rs");
    expect(cleanExtension("")).toBe("");
  });

  test("normalizeFilePath strips file:// scheme while preserving absolute path", () => {
    expect(normalizeFilePath("file:///tmp/image.png")).toBe("/tmp/image.png");
    expect(normalizeFilePath("file://home/gio/code.ts")).toBe("/home/gio/code.ts");
    expect(normalizeFilePath("file:C:/Users/file.txt")).toBe("C:/Users/file.txt");
    expect(normalizeFilePath("/var/log/syslog")).toBe("/var/log/syslog");
  });

  test("image, video, and markdown detection", () => {
    expect(isImageExtension("png")).toBe(true);
    expect(isImageExtension("image.WEBP")).toBe(true);
    expect(isImageExtension("svg")).toBe(true);
    expect(isImageExtension("main.rs")).toBe(false);

    expect(isVideoExtension("webm")).toBe(true);
    expect(isVideoExtension("movie.MP4")).toBe(true);
    expect(isVideoExtension("clip.mov:10")).toBe(true);
    expect(isVideoExtension("image.png")).toBe(false);

    expect(isSvgExtension("svg")).toBe(true);
    expect(isSvgExtension("icon.SVG:10")).toBe(true);
    expect(isSvgExtension("png")).toBe(false);

    expect(isMarkdownExtension("md")).toBe(true);
    expect(isMarkdownExtension("doc.markdown")).toBe(true);
    expect(isMarkdownExtension("plain.txt")).toBe(false);
  });

  test("mimeTypeForExtension returns expected mime types", () => {
    expect(mimeTypeForExtension("png")).toBe("image/png");
    expect(mimeTypeForExtension("svg")).toBe("image/svg+xml");
    expect(mimeTypeForExtension("webp")).toBe("image/webp");
    expect(mimeTypeForExtension("webm")).toBe("video/webm");
    expect(mimeTypeForExtension("mp4")).toBe("video/mp4");
    expect(mimeTypeForExtension("mov")).toBe("video/quicktime");
    expect(mimeTypeForExtension("jpg")).toBe("image/jpeg");
    expect(mimeTypeForExtension("unknown")).toBe("image/png");
  });

  test("formatLanguageLabel maps extensions to friendly titles", () => {
    expect(formatLanguageLabel("ts")).toBe("TYPESCRIPT");
    expect(formatLanguageLabel("dart:239-245")).toBe("DART");
    expect(formatLanguageLabel("rs")).toBe("RUST");
    expect(formatLanguageLabel("")).toBe("TEXT");
    expect(formatLanguageLabel("xyz")).toBe("XYZ");
  });
});
