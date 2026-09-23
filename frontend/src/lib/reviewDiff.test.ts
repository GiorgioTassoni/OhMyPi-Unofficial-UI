import { describe, expect, test } from "bun:test";
import { batchReviewPiece, orderedReviewPieces } from "./reviewDiff";

describe("source-ordered file review", () => {
  test("numbered edits are sorted and distant regions get quiet gap markers", () => {
    const pieces = orderedReviewPieces([
      "156|<body>\n157|\n+158|<!-- Site header -->\n158|<header>\n159|<nav>\n167|</nav>\n168|</header>\n205|</body>",
      "12|--text: #e6edf3;\n13|--muted: #8b949e;\n-14|--accent: #58a6ff;\n+14|--accent: #58a6ff; /* links */\n15|}\n16|",
    ]);

    expect(pieces).toEqual([
      { kind: "diff", text: "-14|--accent: #58a6ff;\n+14|--accent: #58a6ff; /* links */" },
      { kind: "gap", lines: 143 },
      { kind: "diff", text: "+158|<!-- Site header -->" },
    ]);
  });

  test("standard unified hunks sort by their new-file location, not tool order", () => {
    expect(orderedReviewPieces([
      "--- a/f.ts\n+++ b/f.ts\n@@ -120,2 +120,2 @@\n old\n+new",
      "--- a/f.ts\n+++ b/f.ts\n@@ -10,3 +10,3 @@\n old\n-old\n+new",
    ])).toEqual([
      { kind: "diff", text: "-old\n+new" },
      { kind: "gap", lines: 109 },
      { kind: "diff", text: "+new" },
    ]);
  });

  test("nearby and repeated edits do not claim omitted lines", () => {
    expect(orderedReviewPieces([
      "@@ -4 +4 @@\n-a\n+b",
      "@@ -4 +4 @@\n-b\n+c",
      "@@ -5 +5 @@\n-c\n+d",
    ])).toEqual([
      { kind: "diff", text: "-a\n+b\n-b\n+c\n-c\n+d" },
    ]);
  });

  test("unlocated diffs stay in arrival order after located hunks", () => {
    expect(orderedReviewPieces(["+unknown first", "@@ -2 +2 @@\n+x", "+unknown second"])).toEqual([
      { kind: "diff", text: "+x" },
      { kind: "diff", text: "+unknown first" },
      { kind: "diff", text: "+unknown second" },
    ]);
  });

  test("context-only payloads never appear as changed code", () => {
    expect(orderedReviewPieces(["12|unchanged\n13|also unchanged"])).toEqual([]);
    expect(batchReviewPiece("--- a/f.ts\n+++ b/f.ts\n context")).toBeNull();
  });

  test("batch review drops context but retains file labels", () => {
    expect(batchReviewPiece("diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1 +1 @@\n old\n-old\n+new")).toEqual({
      kind: "diff",
      text: "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n-old\n+new",
    });
  });
});
