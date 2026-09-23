import { describe, expect, test } from "bun:test";
import { soundForNotification } from "./notificationSound";

describe("notification sounds", () => {
  test("a completed turn and a question have distinct chimes", () => {
    expect(soundForNotification("turn-finished")).toBe("finished");
    expect(soundForNotification("needs-you")).toBe("needs-you");
  });

  test("failures and background jobs do not chime", () => {
    expect(soundForNotification("failed")).toBeNull();
    expect(soundForNotification("job-finished")).toBeNull();
  });
});
