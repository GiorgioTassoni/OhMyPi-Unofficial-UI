/** Keeps disclosure choices when virtualization unmounts an off-screen turn. */
export interface DisclosureMemory {
  get(kind: "turn" | "group" | "call", row: number, defaultOpen: boolean): boolean;
  set(kind: "turn" | "group" | "call", row: number, open: boolean): void;
}

export function createDisclosureMemory(): DisclosureMemory {
  const choices = new Map<string, boolean>();
  return {
    get: (kind, row, defaultOpen) => choices.get(`${kind}:${row}`) ?? defaultOpen,
    set: (kind, row, open) => { choices.set(`${kind}:${row}`, open); },
  };
}
