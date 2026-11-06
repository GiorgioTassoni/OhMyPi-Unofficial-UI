import { z } from "zod";

export const DesignOptionSchema = z.object({
  id: z.string().describe("Unique identifier (e.g. 'hero-v1', 'card-minimal')"),
  label: z.string().describe("Short display name for the option tab (e.g. 'Minimal Dark')"),
  html: z.string().describe("Complete HTML markup of the component (including <style> or Tailwind classes)"),
  css: z.string().optional().describe("Optional CSS styles associated with the component"),
  notes: z.string().optional().describe("Details or design rationale for this option"),
});

export type DesignOption = z.infer<typeof DesignOptionSchema>;

export const DesignChoiceInputSchema = z.object({
  title: z.string().describe("Title or context of the design decision (e.g. 'Navbar Layout Decision')"),
  description: z.string().optional().describe("Explanation of differences or trade-offs between options"),
  options: z.array(DesignOptionSchema).min(2).describe("At least 2 HTML design options to choose from"),
});

export type DesignChoiceInput = z.infer<typeof DesignChoiceInputSchema>;

export const PreviewHtmlInputSchema = z.object({
  title: z.string().describe("Title of the component or page"),
  html: z.string().describe("HTML/CSS markup to render"),
  description: z.string().optional().describe("Optional description of the component"),
});

export type PreviewHtmlInput = z.infer<typeof PreviewHtmlInputSchema>;

export interface ChoiceResult {
  action?: "selected" | "request_alternatives";
  selectedId: string;
  label: string;
  feedback?: string;
}
