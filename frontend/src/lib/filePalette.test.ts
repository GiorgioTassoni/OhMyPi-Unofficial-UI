import { describe, expect, test } from "bun:test";
import { findAtToken } from "./filePalette";

describe("findAtToken", () => {
  test("finds @ at start of draft", () => {
    expect(findAtToken("@")).toEqual({ start: 0, query: "" });
    expect(findAtToken("@main")).toEqual({ start: 0, query: "main" });
    expect(findAtToken("@src/components/Thread")).toEqual({
      start: 0,
      query: "src/components/Thread",
    });
  });

  test("finds @ preceded by whitespace", () => {
    expect(findAtToken("check out @")).toEqual({ start: 10, query: "" });
    expect(findAtToken("check out @ThreadView")).toEqual({
      start: 10,
      query: "ThreadView",
    });
    expect(findAtToken("first line\n@file")).toEqual({
      start: 11,
      query: "file",
    });
  });

  test("finds @ preceded by punctuation delimiter", () => {
    expect(findAtToken("(@main")).toEqual({ start: 1, query: "main" });
    expect(findAtToken("[@main")).toEqual({ start: 1, query: "main" });
    expect(findAtToken('"@main')).toEqual({ start: 1, query: "main" });
  });

  test("returns null when @ is preceded by word characters", () => {
    expect(findAtToken("user@example.com")).toBeNull();
    expect(findAtToken("test@foo")).toBeNull();
  });

  test("returns null when whitespace follows the mention", () => {
    expect(findAtToken("@main.rs ")).toBeNull();
    expect(findAtToken("@main.rs and more")).toBeNull();
    expect(findAtToken("check @a.ts next")).toBeNull();
  });

  test("returns null when no @ is present", () => {
    expect(findAtToken("")).toBeNull();
    expect(findAtToken("hello world")).toBeNull();
    expect(findAtToken("/compact")).toBeNull();
  });
});
