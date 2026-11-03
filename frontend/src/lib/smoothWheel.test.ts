import { describe, expect, test } from "bun:test";
import { smoothWheel } from "./smoothWheel";

describe("smoothWheel", () => {
  test("attaches scroll and wheel listeners and removes them on cleanup", () => {
    const el = document.createElement("div");
    let scrollCount = 0;
    let wheelCount = 0;
    let pointerCount = 0;

    const originalAdd = el.addEventListener.bind(el);
    const originalRemove = el.removeEventListener.bind(el);

    el.addEventListener = (type: string, listener: EventListenerOrEventListenerObject, options?: unknown) => {
      if (type === "scroll") scrollCount++;
      if (type === "wheel") wheelCount++;
      if (type === "pointerdown") pointerCount++;
      originalAdd(type, listener, options as boolean | AddEventListenerOptions);
    };

    el.removeEventListener = (type: string, listener: EventListenerOrEventListenerObject, options?: unknown) => {
      if (type === "scroll") scrollCount--;
      if (type === "wheel") wheelCount--;
      if (type === "pointerdown") pointerCount--;
      originalRemove(type, listener, options as boolean | EventListenerOptions);
    };

    const cleanup = smoothWheel(el);
    expect(scrollCount).toBe(1);
    expect(wheelCount).toBe(1);
    expect(pointerCount).toBe(1);

    cleanup();
    expect(scrollCount).toBe(0);
    expect(wheelCount).toBe(0);
    expect(pointerCount).toBe(0);
  });

  test("ignores small deltaY from trackpad without calling preventDefault", () => {
    const el = document.createElement("div");
    Object.defineProperty(el, "scrollHeight", { value: 2000, configurable: true });
    Object.defineProperty(el, "clientHeight", { value: 500, configurable: true });

    const cleanup = smoothWheel(el);

    const WheelEventCtor = (window as unknown as { WheelEvent: typeof WheelEvent }).WheelEvent;
    const event = new WheelEventCtor("wheel", { deltaY: 25, deltaMode: 0, cancelable: true });
    el.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(false);
    cleanup();
  });

  test("intercepts large deltaY from mouse notch and calls preventDefault", () => {
    const el = document.createElement("div");
    Object.defineProperty(el, "scrollHeight", { value: 2000, configurable: true });
    Object.defineProperty(el, "clientHeight", { value: 500, configurable: true });

    const cleanup = smoothWheel(el);

    const WheelEventCtor = (window as unknown as { WheelEvent: typeof WheelEvent }).WheelEvent;
    const event = new WheelEventCtor("wheel", { deltaY: 155, deltaMode: 0, cancelable: true });
    el.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(true);
    cleanup();
  });

  test("does not prevent default when container has no overflow", () => {
    const el = document.createElement("div");
    Object.defineProperty(el, "scrollHeight", { value: 500, configurable: true });
    Object.defineProperty(el, "clientHeight", { value: 500, configurable: true });

    const cleanup = smoothWheel(el);

    const WheelEventCtor = (window as unknown as { WheelEvent: typeof WheelEvent }).WheelEvent;
    const event = new WheelEventCtor("wheel", { deltaY: 155, deltaMode: 0, cancelable: true });
    el.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(false);
    cleanup();
  });
});
