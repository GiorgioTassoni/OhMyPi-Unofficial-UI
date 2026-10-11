import type { RowSnapshot } from "../bridge";

export interface AgentMessage {
  sender: string;
  recipient: string | null;
  label: string;
  status: "message" | "completed" | "failed" | "cancelled";
  summary: string;
  body: string;
  sections: { title: string; text: string }[];
  raw: string;
}

export function agentMessage(row: RowSnapshot): AgentMessage | null {
  if (row.role !== "custom" && !row.role.startsWith("notice:")) return null;
  let sender: string;
  let recipient: string | null = null;
  let body: string;
  let label = "Message";
  const incoming = row.text.match(/^\s*<irc>\s*Incoming IRC message from agent `([^`]+)`(?:\s*\(reply to [^)]*\))?:\s*([\s\S]*?)\s*<\/irc>\s*$/);
  const relay = row.text.match(/^\[IRC `([^`]+)` → `([^`]+)`\]\s*([\s\S]*)$/);
  const reply = row.text.match(/^\[IRC you → `([^`]+)` \(auto\)\]\s*([\s\S]*)$/);
  if (incoming) {
    sender = incoming[1];
    // The suffix is engine guidance, not part of the agent's authored message.
    body = incoming[2].replace(/\n\s*Sent while waiting\/working\.[\s\S]*$/, "")
      .replace(/\n\s*(?:If response expected, reply via `hub`|Mid-task: context-generated side-channel auto-reply)[\s\S]*$/, "").trim();
  } else if (relay) {
    sender = relay[1];
    recipient = relay[2];
    body = relay[3].trim();
  } else if (reply) {
    sender = "You";
    recipient = reply[1];
    body = reply[2].trim();
    label = "Automatic reply";
  } else return null;

  const sections: AgentMessage["sections"] = [];
  let status: AgentMessage["status"] = "message";
  let duration = "";
  const task = body.match(/^<task-result\b([^>]*)>([\s\S]*)<\/task-result>\s*$/);
  if (task) {
    const state = attribute(task[1], "status");
    status = state === "completed" ? "completed" : /fail/i.test(state) ? "failed"
      : /cancel|abort/i.test(state) ? "cancelled" : "message";
    label = status === "completed" ? "Task completed" : status === "failed" ? "Task failed"
      : status === "cancelled" ? "Task cancelled" : "Task report";
    duration = attribute(task[1], "duration");
    const output = task[2].match(/<output>([\s\S]*)<\/output>/);
    const preview = task[2].match(/<preview\b([^>]*)>([\s\S]*)<\/preview>/);
    body = (output?.[1] ?? preview?.[2] ?? "").trim();
    for (const [tag, title] of [["abort-reason", "Reason"], ["merge-summary", "Merge summary"]]) {
      const text = task[2].match(new RegExp(`<${tag}>([\\s\\S]*)<\\/${tag}>`))?.[1].trim();
      if (text) sections.push({ title, text });
    }
    const fullOutput = preview && attribute(preview[1], "full-output");
    if (fullOutput) sections.push({ title: "Full output", text: fullOutput });
  }

  let report: Record<string, unknown> | null = null;
  try {
    const parsed: unknown = JSON.parse(body);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) report = parsed as Record<string, unknown>;
  } catch { /* Markdown, plain text and truncated JSON remain readable as authored. */ }
  const summary: string[] = [];
  if (report && Object.keys(report).length > 0) {
    for (const [key, value] of Object.entries(report)) {
      if (key !== "status") sections.push({ title: titleFor(key), text: readable(value) });
    }
    if (Array.isArray(report.files_changed)) {
      const count = report.files_changed.length;
      summary.push(`${count} ${count === 1 ? "file" : "files"} changed`);
    }
    const acceptance = report.acceptance;
    if (acceptance && typeof acceptance === "object" && "result" in acceptance && typeof acceptance.result === "string") {
      summary.push(shorten(acceptance.result));
    }
    // Keep status-only/unknown reports visible too, rather than hiding their contents.
    if (sections.length > 0) body = "";
  }
  if (duration) summary.push(duration);
  return { sender, recipient, label, status, summary: summary.join(" · ") || shorten(body) || "View report", body, sections, raw: row.text };
}

function attribute(attributes: string, name: string): string {
  return attributes.match(new RegExp(`\\b${name}="([^"]*)"`))?.[1] ?? "";
}

function shorten(text: string): string {
  const line = text.replace(/\s+/g, " ").trim();
  return line.length > 160 ? `${line.slice(0, 159)}…` : line;
}

function titleFor(key: string): string {
  if (/[/.\\]/.test(key)) return key;
  const title = key.replace(/_/g, " ");
  return title.charAt(0).toUpperCase() + title.slice(1);
}

/** Turn nested report fields into markdown lists, without discarding unknown fields. */
function readable(value: unknown, depth = 0): string {
  if (typeof value === "string") return value;
  if (depth > 8) return JSON.stringify(value, null, 2);
  if (Array.isArray(value)) return value.map((item) => `- ${readable(item, depth + 1).replace(/\n/g, "\n  ")}`).join("\n") || "None";
  if (value && typeof value === "object") return Object.entries(value)
    .map(([key, item]) => `- **${titleFor(key)}:** ${readable(item, depth + 1).replace(/\n/g, "\n  ")}`).join("\n") || "None";
  return String(value);
}
