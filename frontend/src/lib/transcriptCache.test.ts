import { describe, expect, test } from "bun:test";
import { row, said } from "./rows.fixture";
import { initialProjection, projectTranscript } from "./transcriptCache";

describe("incremental transcript projection", () => {
  test("a tail patch reuses old turn and file summary identities", () => {
    const first = [row({ role: "user", text: "one" }), row({ role: "tool", tool: {
      toolCallId: "edit", toolName: "edit", intent: null, args: '{"path":"/one.ts"}',
      details: '{"path":"/one.ts","diff":"@@ -1 +1 @@\\n-a\\n+b"}', output: "", isError: false, finished: true,
    } }), said("done"), row({ role: "user", text: "two" }), said("typing", { streaming: true })];
    const initial = initialProjection(first, true);
    const nextRows = [...first.slice(0, 4), said("finished")];
    const next = projectTranscript(initial, nextRows, false, 4);
    expect(next.turns[0]).toBe(initial.turns[0]);
    expect(next.turns[1]).not.toBe(initial.turns[1]);
    expect(next.summaries.get(2)).toBe(initial.summaries.get(2));
  });

  test("a patch inside an older turn invalidates that turn and every following one", () => {
    const rows = [row({ role: "user" }), said("one"), row({ role: "user" }), said("two")];
    const initial = initialProjection(rows, false);
    const next = projectTranscript(initial, [rows[0], said("revised"), rows[2], rows[3]], false, 1);
    expect(next.turns[0].answer?.row.text).toBe("revised");
    expect(next.turns[0]).not.toBe(initial.turns[0]);
    expect(next.turns[1]).not.toBe(initial.turns[1]);
  });
});
