import { describe, expect, test, beforeEach } from "bun:test";
import {
  readNotificationSoundEnabled,
  saveNotificationSoundEnabled,
  soundForNotification,
} from "./notificationSound";

describe("notification sounds", () => {
  const store = new Map<string, string>();
  const mockStorage = {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => store.set(k, v),
    removeItem: (k: string) => store.delete(k),
    clear: () => store.clear(),
  };

  beforeEach(() => {
    store.clear();
    (globalThis as unknown as { localStorage: typeof mockStorage }).localStorage = mockStorage;
  });

  test("notification sound is enabled by default when unset", () => {
    expect(readNotificationSoundEnabled()).toBe(true);
  });

  test("saving disabled state persists and reads false", () => {
    saveNotificationSoundEnabled(false);
    expect(readNotificationSoundEnabled()).toBe(false);
  });

  test("saving enabled state persists and reads true", () => {
    saveNotificationSoundEnabled(true);
    expect(readNotificationSoundEnabled()).toBe(true);
  });

  test("a completed turn and a question have distinct chimes", () => {
    expect(soundForNotification("turn-finished")).toBe("finished");
    expect(soundForNotification("needs-you")).toBe("needs-you");
  });

  test("failures and background jobs do not chime", () => {
    expect(soundForNotification("failed")).toBeNull();
    expect(soundForNotification("job-finished")).toBeNull();
  });
});
