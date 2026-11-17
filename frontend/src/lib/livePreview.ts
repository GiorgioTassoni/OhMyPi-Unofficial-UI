import type { RowSnapshot, ToolSnapshot } from "../bridge";
import { parseObject } from "./toolView";

const PREVIEW_TOOL = /^(?:xd:\/\/)?mcp__.*_preview_html$/;
const PREVIEW_DEVICE = /^xd:\/\/mcp__.*_preview_html$/;

function asksForPreview(tool: ToolSnapshot): boolean {
  if (PREVIEW_TOOL.test(tool.toolName)) return true;
  if (tool.toolName !== "write") return false;
  const args = parseObject(tool.args);
  return typeof args?.path === "string" && PREVIEW_DEVICE.test(args.path);
}

function payloadUrl(output: string): string | null {
  const payload = parseObject(output);
  if (payload?.status !== "ready" || typeof payload.url !== "string") return null;

  try {
    const url = new URL(payload.url);
    const local = url.hostname === "127.0.0.1" || url.hostname === "localhost";
    return local && (url.protocol === "http:" || url.protocol === "https:") ? payload.url : null;
  } catch {
    return null;
  }
}

export interface PreviewRequest {
  toolCallId: string;
  url: string;
}

export function previewRequestUrl(row: RowSnapshot | undefined): string | null {
  const tool = row?.tool;
  if (!tool || !tool.finished || tool.isError || !asksForPreview(tool)) return null;
  return payloadUrl(tool.output);
}

export function previewToolCallIds(rows: RowSnapshot[]): string[] {
  return rows.flatMap((row) => (row.tool?.finished && asksForPreview(row.tool) ? [row.tool.toolCallId] : []));
}

export function unseenPreviewRequests(rows: RowSnapshot[], seen: Set<string>): PreviewRequest[] {
  const requests: PreviewRequest[] = [];
  const considered = new Set<string>();
  for (const row of rows) {
    const tool = row.tool;
    if (!tool || seen.has(tool.toolCallId) || considered.has(tool.toolCallId)) continue;
    const url = previewRequestUrl(row);
    if (!url) continue;
    considered.add(tool.toolCallId);
    requests.push({ toolCallId: tool.toolCallId, url });
  }
  return requests;
}
