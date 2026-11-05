import { invoke } from "@tauri-apps/api/core";

const STORAGE_KEY = "omp:mcp-design-choice";

export function readDesignChoiceEnabled(): boolean {
  try {
    const val = localStorage.getItem(STORAGE_KEY);
    return val === null ? true : val === "true";
  } catch {
    return true;
  }
}

export function saveDesignChoiceEnabled(enabled: boolean): void {
  try {
    localStorage.setItem(STORAGE_KEY, String(enabled));
  } catch {
  }
}

export async function syncDesignChoiceMcp(
  enabled: boolean,
  workspace?: string | null
): Promise<void> {
  try {
    await invoke("sync_mcp_design_choice", {
      enabled,
      workspace: workspace ?? null,
    });
  } catch {
  }
}
