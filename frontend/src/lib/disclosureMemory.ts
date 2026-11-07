const MAX_DISCLOSURE_ENTRIES = 300;

export interface DisclosureMemory {
  get(kind: "turn" | "group" | "call", row: number, defaultOpen: boolean): boolean;
  set(kind: "turn" | "group" | "call", row: number, open: boolean): void;
}

export function createDisclosureMemory(maxEntries = MAX_DISCLOSURE_ENTRIES): DisclosureMemory {
  const choices = new Map<string, boolean>();
  return {
    get: (kind, row, defaultOpen) => choices.get(`${kind}:${row}`) ?? defaultOpen,
    set: (kind, row, open) => {
      const key = `${kind}:${row}`;
      if (choices.has(key)) {
        choices.delete(key);
      } else if (choices.size >= maxEntries) {
        const oldest = choices.keys().next().value;
        if (oldest !== undefined) {
          choices.delete(oldest);
        }
      }
      choices.set(key, open);
    },
  };
}
