/**
 * What the model picker shows, decided in one place (`docs/12` §7.1).
 *
 * The catalogue is 601 rows and 1.5 MB of JSON that the host caches, so every rule
 * about it — what a query hides, where a favourite sits, which order two providers
 * come in — has to hold on each keystroke over a list this long without the rows
 * jumping around under the cursor. Those rules live here, pure and asserted
 * (`models.test.ts`), rather than inside the component, so that a search that
 * reorders providers or a favourite that vanishes is a failing test instead of a
 * thing to notice by eye in a 600-row list.
 */

/** One catalogue row, the subset a picker renders. */
export interface ModelOption {
  provider: string;
  id: string;
  name: string;
  contextWindow: number | null;
  reasoning: boolean;
  /** `thinking.defaultLevel` — the model's own suggestion, not the session's level. */
  defaultEffort: string | null;
  /** `thinking.efforts` — the levels this model accepts; empty when it does not think. */
  efforts: string[];
}

/** One provider's rows, in the catalogue's own order. */
export interface ProviderGroup {
  provider: string;
  models: ModelOption[];
}

/** The picker's two lists, in the order the popover renders them. */
export interface VisibleModels {
  /** Stored order, resolved against the catalogue; empty while a query is active. */
  favourites: ModelOption[];
  /** Every matching row, grouped by provider. */
  groups: ProviderGroup[];
}

/**
 * `"provider/id"` — the key favourites are stored under and the session reports.
 *
 * Both halves are needed: the engine's own ids carry a slash
 * (`~anthropic/claude-fable-latest`, measured), so an id alone is not unique
 * across gateways, and `provider` alone does not name a model.
 */
export function modelKey(model: { provider?: string | null; id?: string | null }): string {
  // Tolerant on purpose: `get_state.model` can name a provider without an id, and a
  // key is still what the chip compares against. A partial key simply never matches a
  // catalogue row, which is the honest outcome — better than a chip that cannot render
  // because the session reported half a model.
  return `${model.provider ?? ""}/${model.id ?? ""}`;
}

/**
 * The inverse of `modelKey`, or `null` for a key that is not one.
 *
 * Split on the **first** slash, not the last: measured rows look like
 * `openrouter/~anthropic/claude-fable-latest`, where the provider is the one
 * segment before the first slash and the model id is everything after it.
 */
export function parseKey(key: string): { provider: string; modelId: string } | null {
  const slash = key.indexOf("/");
  // A missing slash means no provider; a leading or trailing one means an empty
  // half. Either way there is no model to look up, and a caller that got a
  // half-key back would select whatever id happened to match it.
  if (slash <= 0 || slash === key.length - 1) {
    return null;
  }

  return { provider: key.slice(0, slash), modelId: key.slice(slash + 1) };
}

/**
 * The picker's rows for a query and a favourite list.
 *
 * Three decisions, each with a reason:
 *
 * - **A query hides the Favourites group.** A favourite is a shortcut to a row,
 *   not a separate model, so while filtering that group could only repeat matches
 *   the provider groups already show. Favourited models are still matched through
 *   those groups, so a search still finds them.
 * - **Provider groups always hold every matching row**, favourites included, for
 *   the same reason: hiding a pinned model from its own provider would make the
 *   star a way to *lose* a model, and unstarring would drop it back into a list
 *   the user was no longer looking at.
 * - **Providers keep the catalogue's order** — first appearance wins. The engine
 *   already returns a stable order, and regrouping on every keystroke by any other
 *   rule (alphabetical, by match count) would slide whole groups under the cursor
 *   while the user types.
 */
export function visibleGroups(
  models: ModelOption[],
  query: string,
  favourites: string[],
): VisibleModels {
  const needle = query.trim().toLowerCase();

  if (needle !== "") {
    // `provider` is matched because it is how the user narrows 601 rows to a
    // gateway — typing `openrouter` to get that gateway's own catalogue is the
    // common case, and nothing in a row's name says where it came from.
    return {
      favourites: [],
      groups: groupByProvider(
        models.filter(
          (model) =>
            model.name.toLowerCase().includes(needle) ||
            model.id.toLowerCase().includes(needle) ||
            model.provider.toLowerCase().includes(needle),
        ),
      ),
    };
  }

  const byKey = new Map(models.map((model) => [modelKey(model), model]));
  const pinned: ModelOption[] = [];
  const resolved = new Set<string>();

  for (const key of favourites) {
    const model = byKey.get(key);
    // A key the catalogue does not have (a model the provider dropped, or a
    // catalogue that has not arrived yet) is skipped rather than rendered as a
    // blank row. A key listed twice is one row: the store is a plain string list
    // that can be edited outside the app, and the same model twice is a bug.
    if (model === undefined || resolved.has(key)) {
      continue;
    }
    resolved.add(key);
    pinned.push(model);
  }

  return { favourites: pinned, groups: groupByProvider(models) };
}

/**
 * The star: add to the end, or remove.
 *
 * Appending keeps the Favourites group in the order the user starred things, which
 * is the only order they have expressed; `moveFavourite` is where they revise it.
 */
export function toggleFavourite(favourites: string[], key: string): string[] {
  if (favourites.includes(key)) {
    return favourites.filter((entry) => entry !== key);
  }

  return [...favourites, key];
}

/**
 * Move a favourite `delta` places, clamped to the ends.
 *
 * Clamping is what makes `⌥↑` on the first row and `⌥↓` on the last one no-ops
 * instead of wrapping a model to the far end — a wrap reads as a lost row. It also
 * settles drag-and-drop: the component passes `targetIndex - sourceIndex`, and a
 * drop beyond either end lands on that end, which is where the user dropped it.
 */
export function moveFavourite(favourites: string[], key: string, delta: number): string[] {
  const from = favourites.indexOf(key);
  if (from < 0) {
    // Nothing to move: a key the store does not hold is not silently appended.
    return [...favourites];
  }

  const next = favourites.filter((entry) => entry !== key);
  const to = Math.min(Math.max(from + delta, 0), next.length);
  next.splice(to, 0, key);
  return next;
}

/**
 * The badge in front of a provider's name.
 *
 * `docs/12` §7.1 makes provider glyphs app-owned assets, and none ship yet, so
 * every provider renders its monogram: the initials of a hyphenated name
 * (`amazon-bedrock` → `AB`), else the first two letters (`openrouter` → `OP`).
 *
 * A monogram can collide (`openai` and `openrouter` both start `OP`). That is
 * survivable because the badge sits beside the provider's full name in the group
 * header, which is what the user actually reads; the asset table replaces this
 * function's output for the providers it covers.
 */
export function providerGlyph(provider: string): string {
  const words = provider.split(/[^A-Za-z0-9]+/).filter((word) => word !== "");

  if (words.length === 0) {
    // A row whose provider is missing still needs a badge rather than a hole.
    return "?";
  }

  const initials =
    words.length === 1 ? words[0].slice(0, 2) : words[0].slice(0, 1) + words[1].slice(0, 1);

  return initials.toUpperCase();
}

function groupByProvider(models: ModelOption[]): ProviderGroup[] {
  // A `Map` iterates in insertion order, which is exactly "providers in the order
  // the catalogue first mentioned them".
  const groups = new Map<string, ModelOption[]>();

  for (const model of models) {
    const group = groups.get(model.provider);
    if (group === undefined) {
      groups.set(model.provider, [model]);
    } else {
      group.push(model);
    }
  }

  return [...groups].map(([provider, rows]) => ({ provider, models: rows }));
}
