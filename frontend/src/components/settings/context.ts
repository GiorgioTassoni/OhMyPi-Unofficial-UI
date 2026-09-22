/**
 * What the settings screen hands down to the controls that need more than their own row
 * (`docs/12` §12).
 *
 * Two things in this screen are not about a setting: which session the window is showing —
 * the host reads that thread's workspace to find the project config file — and the model
 * catalogue, which the record editor's picker draws from. Both live in the shell, and the
 * rows that need them are four levels below the screen, so they travel by injection rather
 * than as props threaded through components that would have no use for them.
 *
 * The thread is also provided under the plain string key `"settings-thread"`, which is the
 * interface the raw-config panel reads: a self-contained component that takes no props has
 * no other way to know which workspace's file it is looking at.
 */

import type { InjectionKey, Ref } from "vue";

import type { ModelOption } from "../../bridge";

export interface SettingsContext {
  /** The session whose workspace the host treats as the one in view; `null` with nothing open. */
  thread: Ref<string | null>;
  /** The active session's approval mode, for the General section's mode row. */
  mode: Ref<string | null>;
  /** The cached catalogue, for the record editor's picker. */
  models: Ref<ModelOption[]>;
  /** Favourite model keys, in the user's order. */
  favourites: Ref<string[]>;
  refreshing: Ref<boolean>;
  /** Store the favourites order; the answer is the host's. */
  setFavourites: (keys: string[]) => void;
}

export const SETTINGS_CONTEXT: InjectionKey<SettingsContext> = Symbol("settings-context");

/** The string key the raw-config panel reads its thread from. */
export const SETTINGS_THREAD = "settings-thread";
