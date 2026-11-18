import { invoke } from "./core";

export interface SkillInfoDto {
  name: string;
  description: string;
  path: string;
  scope: "user" | "project" | "claude";
  hasScripts: boolean;
  hasReferences: boolean;
}

export interface DetectedSkillDto {
  name: string;
  description: string;
  relativePath: string;
  hasScripts: boolean;
  hasReferences: boolean;
  contentPreview: string;
}

export interface InspectSkillResultDto {
  repoName: string;
  skills: DetectedSkillDto[];
}

export interface CreateSkillInputDto {
  name: string;
  description: string;
  content: string;
  scope: "user" | "project";
  workspace?: string | null;
}

export interface ImportSkillInputDto {
  sourceUrl: string;
  skillName?: string | null;
  scope: "user" | "project";
  workspace?: string | null;
}

/** List all installed and discovered skills (user, project, and Claude). */
export async function listSkills(workspace?: string | null): Promise<SkillInfoDto[]> {
  return invoke<SkillInfoDto[]>("list_skills", { workspace: workspace ?? null });
}

/** Create a custom skill from scratch. */
export async function createCustomSkill(input: CreateSkillInputDto): Promise<SkillInfoDto> {
  return invoke<SkillInfoDto>("create_custom_skill", { input });
}

/** Inspect a remote GitHub repository or URL for available skills. */
export async function inspectRemoteSkill(sourceUrl: string): Promise<InspectSkillResultDto> {
  return invoke<InspectSkillResultDto>("inspect_remote_skill", { sourceUrl });
}

/** Import a remote skill from GitHub into global or project skills. */
export async function importRemoteSkill(input: ImportSkillInputDto): Promise<SkillInfoDto> {
  return invoke<SkillInfoDto>("import_remote_skill", { input });
}

/** Open skills directory in the system file manager. */
export async function openSkillsFolder(
  scope: "user" | "project",
  workspace?: string | null,
): Promise<void> {
  return invoke<void>("open_skills_folder", { scope, workspace: workspace ?? null });
}

