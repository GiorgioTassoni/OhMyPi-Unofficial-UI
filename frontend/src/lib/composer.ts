/**
 * The composer's keyboard map, as a pure function.
 *
 * `docs/12` §5.2 fixes what each key does, and the mapping is the part of a
 * composer that is easy to get subtly wrong — a `⇧Enter` that sends, an empty
 * prompt that fires, or `Enter` during IME composition, which sends half a word for
 * every CJK and IME user. Kept out of the component so those cases are asserted
 * rather than trusted to a reviewer's eye (`composer.test.ts`).
 */

/** What a key press asks the composer to do. */
export type ComposerOperation =
  /** Start a turn, or answer in an idle session. */
  | "prompt"
  /** Inject into the running turn. */
  | "steer"
  /** Queue behind the running turn. */
  | "follow-up"
  /** Insert a line break — the text area's own behaviour. */
  | "newline"
  /** Stop the running turn. */
  | "abort"
  /** Discard the draft. */
  | "clear"
  /** Run the palette's selected row: dispatch it, or finish its name. */
  | "palette-accept"
  /** Complete the selected row's name into the box without running anything. */
  | "palette-complete"
  /** Dismiss the palette, leaving the text alone. */
  | "palette-close"
  /** Walk the palette's rows. */
  | "palette-up"
  | "palette-down"
  /** Nothing: let the key through untouched. */
  | "none";

export interface ComposerKey {
  key: string;
  alt?: boolean;
  shift?: boolean;
  /** True while an IME is composing: `Enter` commits a character, it does not send. */
  composing?: boolean;
}

export interface ComposerState {
  /**
   * Whether a turn is in flight, as `get_state` last reported.
   *
   * A frame stale is fine: measured at v18.2.6, the engine accepts a steering
   * message when no turn is running and starts one, so being wrong costs nothing.
   */
  streaming: boolean;
  draft: string;
  /**
   * How many attachments the composer is holding.
   *
   * Part of the state because `docs/12` §5.1 makes a picture with no words a message:
   * measured at v18.2.6, `prompt` with an empty `message` and one image answers
   * `success` and starts a real turn. So "empty input never sends" is about the text
   * *and* what is attached to it.
   */
  attachments: number;
  /**
   * Whether the palette is showing (`docs/12` §7.3).
   *
   * The composer derives this from the draft and the caret (`lib/palette.ts`), so it is a
   * consequence of what is in the box rather than a mode someone has to keep in step.
   * While it is up it takes `Enter`, `Tab`, `Escape` and the arrows; a modifier escapes
   * it, so `⌥Enter` still queues behind a running turn.
   */
  palette: boolean;
}

export function operationFor(key: ComposerKey, state: ComposerState): ComposerOperation {
  // Before anything else: while composing, every key belongs to the IME.
  if (key.composing) {
    return "none";
  }

  // Whitespace is not a message, and neither is an empty composer. `docs/12` §5.2:
  // empty input never sends — where "empty" means no text *and* nothing attached.
  const hasInput = state.draft.trim() !== "" || state.attachments > 0;

  // The palette is up: the keys that would otherwise send, clear or move the caret belong
  // to it. `Tab` completes rather than running anything — completing is the safe half of
  // accepting, and the one a user reaches for when unsure.
  if (state.palette) {
    switch (key.key) {
      case "ArrowDown":
        return "palette-down";
      case "ArrowUp":
        return "palette-up";
      case "Tab":
        return key.shift ? "none" : "palette-complete";
      case "Escape":
        return "palette-close";
      case "Enter":
        if (key.shift) {
          return "newline";
        }
        // A modifier is the way out: `⌥Enter` keeps meaning "queue behind the turn".
        if (key.alt) {
          break;
        }
        return "palette-accept";
      default:
        return "none";
    }
  }

  if (key.key === "Enter") {
    if (key.shift) {
      return "newline";
    }
    if (!hasInput) {
      return "none";
    }
    if (state.streaming) {
      // `⌥Enter` queues; plain `Enter` steers into the turn.
      return key.alt ? "follow-up" : "steer";
    }

    // While idle the modifier has no meaning: the message is a prompt either way.
    return "prompt";
  }

  if (key.key === "Escape") {
    // While a turn is running, stopping it is the more urgent reading — and the
    // draft survives, so nothing typed is lost by it.
    // Clearing while idle discards the whole draft, attachments included: the chip
    // strip is part of what the user is composing.
    return state.streaming ? "abort" : hasInput ? "clear" : "none";
  }

  return "none";
}
