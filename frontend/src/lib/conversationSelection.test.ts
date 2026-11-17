import { describe, expect, test } from "bun:test";
import { attachConversationSelection, hasSelectionWithin } from "./conversationSelection";

function mountConversation() {
  const root = document.createElement("div");
  root.setAttribute("data-conversation", "true");
  const greeting = document.createElement("div");
  greeting.setAttribute("data-greeting", "true");
  const plane = document.createElement("div");
  plane.setAttribute("data-selection-surface", "");
  const column = document.createElement("div");
  column.className = "max-w-[46rem] select-text";
  const first = document.createElement("p");
  first.textContent = "First line";
  const second = document.createElement("p");
  second.textContent = "Second line";
  const button = document.createElement("button");
  button.textContent = "Copy";
  const link = document.createElement("a");
  link.href = "#";
  link.textContent = "Link";
  const input = document.createElement("input");
  column.append(first, second, button, link, input);
  plane.append(column);
  root.append(greeting, plane);
  document.body.append(root);
  window.getSelection()?.removeAllRanges();
  return {
    root,
    greeting,
    plane,
    column,
    first,
    firstText: first.firstChild as Text,
    button,
    link,
    input,
  };
}

function press(target: EventTarget, init: MouseEventInit = {}): MouseEvent {
  const event = new window.MouseEvent("mousedown", { bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
}

describe("attachConversationSelection", () => {
  test("stops a blank press on the selection plane without intercepting the scroll root", () => {
    const view = mountConversation();
    const detach = attachConversationSelection(view.root);

    expect(press(view.root).defaultPrevented).toBe(false);
    expect(press(view.plane).defaultPrevented).toBe(true);

    detach();
    view.root.remove();
  });

  test("leaves text, controls, other buttons, and the keyboard to the browser", () => {
    const view = mountConversation();
    const detach = attachConversationSelection(view.root);

    expect(press(view.firstText).defaultPrevented).toBe(false);
    expect(press(view.first).defaultPrevented).toBe(false);
    expect(press(view.greeting).defaultPrevented).toBe(false);
    expect(press(view.button).defaultPrevented).toBe(false);
    expect(press(view.link).defaultPrevented).toBe(false);
    expect(press(view.input).defaultPrevented).toBe(false);
    expect(press(view.plane, { button: 1 }).defaultPrevented).toBe(false);
    expect(press(view.plane, { button: 2 }).defaultPrevented).toBe(false);

    const key = new window.KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: "a" });
    view.root.dispatchEvent(key);
    expect(key.defaultPrevented).toBe(false);

    detach();
    view.root.remove();
  });

  test("clears a previous selection when the blank surface is pressed", () => {
    const view = mountConversation();
    const detach = attachConversationSelection(view.root);
    const selection = window.getSelection()!;
    selection.setBaseAndExtent(view.firstText, 0, view.firstText, 10);

    press(view.plane);

    expect(selection.rangeCount).toBe(0);
    expect(hasSelectionWithin(view.root)).toBe(false);

    detach();
    view.root.remove();
  });

  test("tears down idempotently", () => {
    const view = mountConversation();
    const detach = attachConversationSelection(view.root);

    detach();
    detach();

    expect(press(view.plane).defaultPrevented).toBe(false);
    expect(press(view.root).defaultPrevented).toBe(false);
    view.root.remove();
  });

});

function mountMessages() {
  const root = document.createElement("div");
  const messages = [0, 1, 2].map(() => {
    const message = document.createElement("article");
    const paragraph = document.createElement("p");
    paragraph.textContent = "Message text";
    message.append(paragraph);
    root.append(message);
    return { message, text: paragraph.firstChild as Text };
  });
  document.body.append(root);
  window.getSelection()?.removeAllRanges();
  return { root, first: messages[0], middle: messages[1], last: messages[2] };
}

describe("hasSelectionWithin", () => {
  test("is false without a container, a range, or a non-collapsed range", () => {
    const view = mountMessages();
    const selection = window.getSelection()!;

    expect(hasSelectionWithin(null)).toBe(false);
    expect(hasSelectionWithin(view.root)).toBe(false);

    selection.collapse(view.middle.text, 3);
    expect(hasSelectionWithin(view.middle.message)).toBe(false);

    view.root.remove();
  });

  test("sees a selection inside the container", () => {
    const view = mountMessages();
    window.getSelection()!.setBaseAndExtent(view.first.text, 0, view.first.text, 7);

    expect(hasSelectionWithin(view.first.message)).toBe(true);
    expect(hasSelectionWithin(view.root)).toBe(true);

    view.root.remove();
  });

  test("sees a selection spanning a container its endpoints sit outside", () => {
    const view = mountMessages();
    window.getSelection()!.setBaseAndExtent(view.first.text, 0, view.last.text, 7);

    expect(hasSelectionWithin(view.middle.message)).toBe(true);
    expect(hasSelectionWithin(view.first.message)).toBe(true);
    expect(hasSelectionWithin(view.last.message)).toBe(true);

    view.root.remove();
  });

  test("ignores selections outside the container and cleared ones", () => {
    const view = mountMessages();
    const outside = document.createElement("section");
    outside.textContent = "Elsewhere";
    document.body.append(outside);
    const selection = window.getSelection()!;

    selection.setBaseAndExtent(outside.firstChild!, 0, outside.firstChild!, 4);
    expect(hasSelectionWithin(view.middle.message)).toBe(false);
    expect(hasSelectionWithin(view.root)).toBe(false);

    selection.setBaseAndExtent(view.first.text, 0, view.last.text, 7);
    selection.removeAllRanges();
    expect(hasSelectionWithin(view.root)).toBe(false);

    outside.remove();
    view.root.remove();
  });
});
