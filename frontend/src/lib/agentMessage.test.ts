import { expect, test } from "bun:test";
import { row } from "./rows.fixture";
import { agentMessage } from "./agentMessage";

const report = `<irc>
Incoming IRC message from agent \`AppFixes\` (reply to abc123):

<task-result id="AppFixes" agent="AppFixes" status="completed" duration="25.6s">
<meta lines="40" size="4.8KB" />
<output>{"status":"complete","files_changed":["mdw/app.py","tests/test_app.py"],"acceptance":{"command":"pytest tests/test_app.py","result":"23 passed (was 18)"},"changes":{"mdw/app.py":["Fixed menu callbacks","Preserved dirty state"]},"warnings":["Existing renderer failure"],"extra_field":"Keep this too"}</output>
</task-result>

Sent while waiting/working. Active interruptible wait stopped early for immediate reading.

If response expected, reply via \`hub\` (\`op: "send"\`, \`to: "AppFixes"\`); may finish current step first. No one replies on your behalf.
</irc>`;

test("incoming task reports expose useful metadata and readable sections, not their transport wrapper", () => {
  const message = agentMessage(row({ role: "custom", customType: "irc:incoming", text: report }));
  expect(message?.sender).toBe("AppFixes");
  expect(message?.status).toBe("completed");
  expect(message?.summary).toBe("2 files changed · 23 passed (was 18) · 25.6s");
  expect(message?.sections).toContainEqual({ title: "Extra field", text: "Keep this too" });
  expect(message?.sections.find((section) => section.title === "Files changed")?.text).toContain("mdw/app.py");
  expect(message?.sections.find((section) => section.title === "Changes")?.text).toContain("Fixed menu callbacks");
  expect(message?.sections.find((section) => section.title === "Changes")?.text).toContain("**mdw/app.py:**");
  expect(message?.body).not.toContain("reply via");
  expect(message?.raw).toBe(report);
});

test("plain IRC messages keep markdown, with a bounded single-line preview", () => {
  const body = ("**Need your input**\n\n" + "A long explanation. ".repeat(30)).trim();
  const message = agentMessage(row({ role: "custom", text: `<irc>\nIncoming IRC message from agent \`Scout\`:\n\n${body}\n\nIf response expected, reply via \`hub\` blah\n</irc>` }));
  expect(message?.sender).toBe("Scout");
  expect(message?.body).toBe(body);
  expect(message?.summary.length).toBeLessThanOrEqual(160);
  expect(message?.summary).not.toContain("\n");
});

test("relay messages keep both endpoints", () => {
  const message = agentMessage(row({ role: "custom", customType: "irc:relay", text: "[IRC `Parser` → `Renderer`]\n\nThe interface is ready." }));
  expect(message?.sender).toBe("Parser");
  expect(message?.recipient).toBe("Renderer");
  expect(message?.body).toBe("The interface is ready.");
});

test("automatic replies are distinguished from incoming messages", () => {
  const message = agentMessage(row({ role: "custom", customType: "irc:autoreply", text: "[IRC you → `Scout` (auto)]\n\nPlease keep working." }));
  expect(message?.sender).toBe("You");
  expect(message?.recipient).toBe("Scout");
  expect(message?.label).toBe("Automatic reply");
});

test("a failed truncated report retains its preview, failure reason and full-output pointer", () => {
  const text = '<irc>Incoming IRC message from agent `Tests`:\n<task-result status="failed (exit 1)" duration="1m 2s"><abort-reason>Budget reached</abort-reason><preview full-output="agent://Tests">Tests failed\n{broken json</preview></task-result></irc>';
  const message = agentMessage(row({ role: "custom", text }));
  expect(message?.status).toBe("failed");
  expect(message?.body).toContain("{broken json");
  expect(message?.sections).toContainEqual({ title: "Reason", text: "Budget reached" });
  expect(message?.sections).toContainEqual({ title: "Full output", text: "agent://Tests" });
  expect(message?.raw).toBe(text);
});

test("ordinary messages mentioning IRC are not transformed", () => {
  expect(agentMessage(row({ text: "Use <irc> to send a message" }))).toBeNull();
  expect(agentMessage(row({ role: "user", text: report }))).toBeNull();
});
