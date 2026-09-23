import { describe, expect, test } from "bun:test";
import { APP_THEMES, DEFAULT_APP_THEME, parseAppTheme } from "./appTheme";

describe("app themes", () => {
  test("keeps every offered theme selectable", () => {
    expect(APP_THEMES.map((theme) => parseAppTheme(theme.id))).toEqual(APP_THEMES.map((theme) => theme.id));
  });

  test("unknown and absent saved themes fall back to the original palette", () => {
    expect(parseAppTheme(null)).toBe(DEFAULT_APP_THEME);
    expect(parseAppTheme("an-old-theme")).toBe(DEFAULT_APP_THEME);
  });
});
