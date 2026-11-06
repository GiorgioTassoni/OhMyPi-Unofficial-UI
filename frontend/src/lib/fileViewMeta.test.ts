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
  stripToolOutputFormatting,
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

    const urlMeta = parseFilePath("https://webassets.linear.app/images/prod/sample.png?q=95&auto=format");
    expect(urlMeta.cleanPath).toBe("https://webassets.linear.app/images/prod/sample.png?q=95&auto=format");
    expect(urlMeta.basename).toBe("sample.png");
    expect(urlMeta.directory).toBe("https://webassets.linear.app/images/prod");
    expect(urlMeta.extension).toBe("png");
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
    expect(cleanExtension("png?q=95&auto=format&dpr=2")).toBe("png");
    expect(cleanExtension(".rs")).toBe("rs");
    expect(cleanExtension("")).toBe("");
  });

  test("normalizeFilePath strips file:// scheme while preserving absolute path", () => {
    expect(normalizeFilePath("file:///tmp/image.png")).toBe("/tmp/image.png");
    expect(normalizeFilePath("file://home/gio/code.ts")).toBe("/home/gio/code.ts");
    expect(normalizeFilePath("file:C:/Users/file.txt")).toBe("C:/Users/file.txt");
    expect(normalizeFilePath("/var/log/syslog")).toBe("/var/log/syslog");
    expect(
      normalizeFilePath(
        "file:///home/user/docs/UI%20Component%20Schema%20v1.0.txt",
      ),
    ).toBe("/home/user/docs/UI Component Schema v1.0.txt");
    expect(
      normalizeFilePath("docs/UI%20Component%20Schema%20v1.0.txt"),
    ).toBe("docs/UI Component Schema v1.0.txt");
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

  test("stripToolOutputFormatting removes tool headers and line numbers", () => {
    const toolOutput = "[~/Projects/Websites_or_webapp/Assetty/AGENT.md#C1FA]\n1:# Instructions for Working on the Project\n2:\n3:This is the single source\n4:\n5:## Coding rules";
    const expected = "# Instructions for Working on the Project\n\nThis is the single source\n\n## Coding rules";
    expect(stripToolOutputFormatting(toolOutput)).toBe(expected);

    const pipeOutput = " 1 | line one\n 2 | line two\n 3 | line three";
    expect(stripToolOutputFormatting(pipeOutput)).toBe("line one\nline two\nline three");

    const singleLineWithHeader = "[file.txt#1234]\n1:hello world";
    expect(stripToolOutputFormatting(singleLineWithHeader)).toBe("hello world");

    const markdownList = "# Title\n\n1. First item\n2. Second item\n3. Third item";
    expect(stripToolOutputFormatting(markdownList)).toBe(markdownList);

    const singleLineRatio = "1: 100";
    expect(stripToolOutputFormatting(singleLineRatio)).toBe(singleLineRatio);

    expect(stripToolOutputFormatting("")).toBe("");
  });
});
