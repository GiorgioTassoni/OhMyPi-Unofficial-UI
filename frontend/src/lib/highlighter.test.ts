import { describe, expect, it } from "bun:test";
import { escapeHtml, highlightCode } from "./highlighter";

describe("highlightCode", () => {
  it("escapes empty code as a non-breaking space", () => {
    expect(highlightCode("", "ts")).toEqual(["&nbsp;"]);
  });

  it("escapes raw HTML entities safely", () => {
    expect(escapeHtml("<script>alert('xss')</script>")).toBe("&lt;script>alert('xss')&lt;/script>");
  });

  it("highlights Dart code with Prism tags", () => {
    const dartCode = `Widget build(BuildContext context) {
  final theme = Theme.of(context);
  @override
  height: 56,
}`;
    const lines = highlightCode(dartCode, "dart");
    expect(lines.length).toBe(5);
    expect(lines[0]).toContain('class="token class-name">Widget</span>');
    expect(lines[0]).toContain('class="token function">build</span>');
    expect(lines[0]).toContain('class="token class-name">BuildContext</span>');

    expect(lines[1]).toContain('class="token keyword">final</span>');
    expect(lines[1]).toContain('class="token class-name">Theme</span>');
    expect(lines[1]).toContain('class="token function">of</span>');

    expect(lines[2]).toContain('class="token metadata function">@override</span>');
    expect(lines[3]).toContain('class="token number">56</span>');
  });

  it("handles paths with line range suffixes like dart:239-245", () => {
    const dartCode = `class _EditorCanvasState extends State<EditorCanvas> {\n  @override\n  Widget build(BuildContext context) {\n    return ColoredBox(\n      child: LayoutBuilder(\n      );\n    );\n  }\n}`;
    const lines = highlightCode(dartCode, "dart:239-245");
    expect(lines[0]).toContain('class="token keyword">class</span>');
    expect(lines[0]).toContain('class="token class-name">State</span>');
    expect(lines[0]).toContain('class="token class-name">EditorCanvas</span>');
    expect(lines[1]).toContain('class="token metadata function">@override</span>');
    expect(lines[2]).toContain('class="token class-name">Widget</span>');
    expect(lines[3]).toContain('class="token class-name">ColoredBox</span>');
    expect(lines[4]).toContain('class="token property">child</span>');
    expect(lines[4]).toContain('class="token class-name">LayoutBuilder</span>');
  });

  it("splits multiline comments while preserving token tags on each line", () => {
    const code = `/* line 1\nline 2 */\nvar x = 1;`;
    const lines = highlightCode(code, "dart");
    expect(lines.length).toBe(3);
    expect(lines[0]).toBe('<span class="token comment">/* line 1</span>');
    expect(lines[1]).toBe('<span class="token comment">line 2 */</span>');
    expect(lines[2]).toContain('class="token keyword">var</span>');
  });

  it("falls back gracefully on unknown extension", () => {
    const lines = highlightCode("hello world\nfoo bar", "xyz123");
    expect(lines).toEqual(["hello world", "foo bar"]);
  });
});
