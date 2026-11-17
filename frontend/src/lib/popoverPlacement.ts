// Only ancestor scrolling moves the trigger; resize targets Window rather than a Node.
export function needsPlacement(event: Event, panel: Element | null, trigger: Element | null): boolean {
  const target = event.target;
  if (!(target instanceof Node)) return true;
  if (panel?.contains(target) === true) return false;
  if (event.type === "scroll" && (trigger === null || !target.contains(trigger))) return false;
  return true;
}
