import { z } from "zod";

export const DesignOptionSchema = z.object({
  id: z.string().describe("Identificativo univoco (es. 'hero-v1', 'card-minimal')"),
  label: z.string().describe("Nome sintetico dell'opzione per la scheda (es. 'Minimal Dark')"),
  html: z.string().describe("Codice HTML completo del componente (inclusi stili <style> o classi Tailwind)"),
  css: z.string().optional().describe("CSS opzionale associato"),
  notes: z.string().optional().describe("Dettagli o motivazione di questa scelta di design"),
});

export type DesignOption = z.infer<typeof DesignOptionSchema>;

export const DesignChoiceInputSchema = z.object({
  title: z.string().describe("Titolo o contesto della scelta per l'utente (es. 'Scelta layout Navbar')"),
  description: z.string().optional().describe("Spiegazione delle differenze o trade-off tra le opzioni"),
  options: z.array(DesignOptionSchema).min(2).describe("Almeno 2 opzioni HTML tra cui scegliere"),
});

export type DesignChoiceInput = z.infer<typeof DesignChoiceInputSchema>;

export const PreviewHtmlInputSchema = z.object({
  title: z.string().describe("Titolo del componente o della pagina"),
  html: z.string().describe("Codice HTML/CSS da visualizzare"),
  description: z.string().optional().describe("Spiegazione del componente"),
});

export type PreviewHtmlInput = z.infer<typeof PreviewHtmlInputSchema>;

export interface ChoiceResult {
  action?: "selected" | "request_alternatives";
  selectedId: string;
  label: string;
  feedback?: string;
}
