import { describe, expect, test } from "bun:test";
import type { ToolSnapshot } from "../bridge";
import { row, said } from "./rows.fixture";
import { previewRequestUrl, previewToolCallIds, unseenPreviewRequests } from "./livePreview";

function call(toolName: string, output: string, args = "", extra: Partial<ToolSnapshot> = {}) {
  return row({
    role: "tool",
    tool: {
      toolCallId: "call-1",
      toolName,
      intent: null,
      args,
      details: "",
      output,
      isError: false,
      finished: true,
      ...extra,
    },
  });
}

const PREVIEW_OUTPUT = JSON.stringify({ status: "ready", url: "http://127.0.0.1:41234", title: "Card" });
const PREVIEW_ARGS = JSON.stringify({ path: "xd://mcp__design_choice_preview_html", content: '{"title":"Card"}' });

describe("live preview requests", () => {
  test("free text and unrelated tools never open a local URL", () => {
    expect(previewRequestUrl(call("bash", "Local: http://127.0.0.1:5173/"))).toBeNull();
    expect(previewRequestUrl(call("mcp__puppeteer_navigate", "http://localhost:8080/tunnel"))).toBeNull();
    expect(previewRequestUrl(said("Ready at http://localhost:3000"))).toBeNull();
    expect(previewRequestUrl(undefined)).toBeNull();
  });

  test("design_choice results and feedback are never preview requests", () => {
    const selected = JSON.stringify({
      status: "selected",
      selectedId: "hero",
      label: "Hero",
      userFeedback: "Use http://127.0.0.1:41234",
    });
    const rejected = JSON.stringify({
      status: "rejected_request_alternatives",
      action: "request_alternatives",
      userFeedback: "Try http://localhost:9000/next",
    });

    expect(previewRequestUrl(call("mcp__design_choice_design_choice", selected))).toBeNull();
    expect(previewRequestUrl(call("mcp__design_choice__design_choice", rejected))).toBeNull();
    expect(
      previewRequestUrl(
        call("write", selected, JSON.stringify({ path: "xd://mcp__design_choice_design_choice", content: "{}" })),
      ),
    ).toBeNull();
    expect(
      previewRequestUrl(
        call("write", PREVIEW_OUTPUT, JSON.stringify({ path: "xd://mcp__design_choice_design_choice", content: "{}" })),
      ),
    ).toBeNull();
  });

  test("a finished preview_html result opens only its structured ready URL", () => {
    expect(previewRequestUrl(call("mcp__design_choice_preview_html", PREVIEW_OUTPUT))).toBe("http://127.0.0.1:41234");
    expect(previewRequestUrl(call("mcp__design_choice__preview_html", PREVIEW_OUTPUT))).toBe("http://127.0.0.1:41234");
    expect(previewRequestUrl(call("mcp__design-choice__preview_html", PREVIEW_OUTPUT))).toBe("http://127.0.0.1:41234");
    expect(previewRequestUrl(call("xd://mcp__design_choice_preview_html", PREVIEW_OUTPUT))).toBe("http://127.0.0.1:41234");
    expect(
      previewRequestUrl(
        call("mcp__design_choice_preview_html", JSON.stringify({ status: "ready", url: "http://localhost:4321" })),
      ),
    ).toBe("http://localhost:4321");
  });

  test("prose, pending, failed, malformed, and non-web payloads open nothing", () => {
    expect(previewRequestUrl(call("mcp__design_choice_preview_html", "Live preview: http://127.0.0.1:41234"))).toBeNull();
    expect(
      previewRequestUrl(call("mcp__design_choice_preview_html", JSON.stringify({ status: "pending", url: "http://127.0.0.1:41234" }))),
    ).toBeNull();
    expect(
      previewRequestUrl(call("mcp__design_choice_preview_html", JSON.stringify({ status: "error", url: "http://127.0.0.1:41234" }))),
    ).toBeNull();
    expect(
      previewRequestUrl(call("mcp__design_choice_preview_html", JSON.stringify({ status: "ready", url: 41234 }))),
    ).toBeNull();
    expect(
      previewRequestUrl(call("mcp__design_choice_preview_html", JSON.stringify({ status: "ready", url: "file:///tmp/preview.html" }))),
    ).toBeNull();
    expect(
      previewRequestUrl(call("mcp__design_choice_preview_html", JSON.stringify({ status: "ready", url: "https://example.com/preview" }))),
    ).toBeNull();
    expect(previewRequestUrl(call("mcp__design_choice_preview_html", PREVIEW_OUTPUT, "", { finished: false }))).toBeNull();
    expect(previewRequestUrl(call("mcp__design_choice_preview_html", PREVIEW_OUTPUT, "", { isError: true }))).toBeNull();
  });

  test("a write to the mounted preview device opens its structured ready URL", () => {
    expect(previewRequestUrl(call("write", PREVIEW_OUTPUT, PREVIEW_ARGS))).toBe("http://127.0.0.1:41234");
  });

  test("writes that only mention a preview device or URL open nothing", () => {
    const fileArgs = JSON.stringify({ path: "src/app.ts", content: "// http://localhost:3000" });
    expect(previewRequestUrl(call("write", "Local: http://127.0.0.1:5173/", fileArgs))).toBeNull();

    const otherDevice = JSON.stringify({ path: "xd://mcp__puppeteer_navigate", content: "{}" });
    expect(previewRequestUrl(call("write", "http://localhost:8080/", otherDevice))).toBeNull();

    const inContent = JSON.stringify({ path: "notes.md", content: "xd://mcp__design_choice_preview_html" });
    expect(previewRequestUrl(call("write", PREVIEW_OUTPUT, inContent))).toBeNull();

    expect(previewRequestUrl(call("read", PREVIEW_OUTPUT, PREVIEW_ARGS))).toBeNull();
  });

  test("a completed preview in a batch is found even behind an assistant tail", () => {
    const requests = unseenPreviewRequests(
      [call("mcp__design_choice_preview_html", PREVIEW_OUTPUT), said("Preview is ready.")],
      new Set<string>(),
    );

    expect(requests).toEqual([{ toolCallId: "call-1", url: "http://127.0.0.1:41234" }]);
  });

  test("loaded history is seeded and later patches never replay it", () => {
    const history = call("mcp__design_choice_preview_html", PREVIEW_OUTPUT);
    const seen = new Set(previewToolCallIds([history]));

    expect(unseenPreviewRequests([history, said("Preview is ready.")], seen)).toEqual([]);

    const completed = call(
      "mcp__design_choice_preview_html",
      JSON.stringify({ status: "ready", url: "http://127.0.0.1:5000" }),
      "",
      { toolCallId: "next" },
    );
    expect(unseenPreviewRequests([history, completed], seen)).toEqual([
      { toolCallId: "next", url: "http://127.0.0.1:5000" },
    ]);

    seen.add("next");
    expect(unseenPreviewRequests([history, completed], seen)).toEqual([]);
  });

  test("a preview pending when history loads opens on its live completion", () => {
    const pending = call("mcp__design_choice_preview_html", "", "", { finished: false });
    const seen = new Set(previewToolCallIds([pending]));
    const finished = call("mcp__design_choice_preview_html", PREVIEW_OUTPUT);

    expect(unseenPreviewRequests([pending], seen)).toEqual([]);
    expect(unseenPreviewRequests([finished, said("Preview is ready.")], seen)).toEqual([
      { toolCallId: "call-1", url: "http://127.0.0.1:41234" },
    ]);
  });

  test("recovery opens only requests completed after the seeded history", () => {
    const history = call("mcp__design_choice_preview_html", PREVIEW_OUTPUT, "", { toolCallId: "old" });
    const finished = call(
      "mcp__design_choice_preview_html",
      JSON.stringify({ status: "ready", url: "http://127.0.0.1:6000" }),
      "",
      { toolCallId: "new" },
    );
    const seen = new Set(previewToolCallIds([history]));

    expect(unseenPreviewRequests([history, finished], seen)).toEqual([
      { toolCallId: "new", url: "http://127.0.0.1:6000" },
    ]);
  });

  test("identity is the tool call, so a second call with the same URL still opens", () => {
    const first = call("mcp__design_choice_preview_html", PREVIEW_OUTPUT, "", { toolCallId: "first" });
    const second = call("mcp__design_choice_preview_html", PREVIEW_OUTPUT, "", { toolCallId: "second" });
    const seen = new Set<string>();

    expect(unseenPreviewRequests([first], seen)).toEqual([{ toolCallId: "first", url: "http://127.0.0.1:41234" }]);
    seen.add("first");
    expect(unseenPreviewRequests([first, second], seen)).toEqual([
      { toolCallId: "second", url: "http://127.0.0.1:41234" },
    ]);

    const changed = call(
      "mcp__design_choice_preview_html",
      JSON.stringify({ status: "ready", url: "http://127.0.0.1:7777" }),
      "",
      { toolCallId: "first" },
    );
    expect(unseenPreviewRequests([first, changed], seen)).toEqual([]);
  });
});
