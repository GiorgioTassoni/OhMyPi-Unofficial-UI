import { onUnmounted, ref, type Ref } from "vue";
import {
  onTerminals,
  terminalClose,
  terminalOpen,
  terminalWrite,
  terminals as readTerminals,
} from "../bridge";
import { selectAfterClose, selectFrom, tabsFrom, type TerminalTab } from "../lib/terminals";

export function useTerminals(options: {
  workspace: Ref<string | null>;
  settings: Ref<boolean>;
  error: Ref<string | null>;
  describe: (cause: unknown) => string;
}) {
  const terminalsOpen = ref(false);
  const terminals = ref<TerminalTab[]>([]);
  const activeTerminal = ref<string | null>(null);
  let unlistenTerminals: (() => void) | null = null;

  onUnmounted(() => {
    unlistenTerminals?.();
    unlistenTerminals = null;
  });

  async function init(): Promise<void> {
    unlistenTerminals = await onTerminals((published) => {
      const before = terminals.value;
      const active = activeTerminal.value;
      terminals.value = tabsFrom(published);
      activeTerminal.value = terminals.value.some((tab) => tab.id === active)
        ? active
        : selectFrom(terminals.value, selectAfterClose(before, active ?? "", active));
    });

    try {
      terminals.value = tabsFrom(await readTerminals());
      activeTerminal.value = selectFrom(terminals.value, activeTerminal.value);
    } catch (cause) {
      options.error.value = options.describe(cause);
    }
  }

  async function openTerminal(cwd: string): Promise<void> {
    if (cwd === "") {
      options.error.value = "there is nowhere to open a terminal: no workspace is open";
      return;
    }

    try {
      const opened = await terminalOpen(cwd, 80, 24);
      terminalsOpen.value = true;
      activeTerminal.value = opened.id;
    } catch (cause) {
      options.error.value = options.describe(cause);
    }
  }

  function selectTerminal(id: string): void {
    activeTerminal.value = id;
  }

  async function closeTerminal(id: string): Promise<void> {
    const next = selectAfterClose(terminals.value, id, activeTerminal.value);

    try {
      terminals.value = tabsFrom(await terminalClose(id));
      activeTerminal.value = next;
    } catch (cause) {
      options.error.value = options.describe(cause);
    }
  }

  function logIn(providerId?: string): void {
    const current = terminals.value.find((tab) => tab.id === activeTerminal.value)?.cwd;
    const cwd = current ?? options.workspace.value ?? "";
    if (cwd === "") {
      options.error.value = "there is nowhere to log in from: no workspace is open";
      return;
    }

    options.settings.value = false;

    void (async () => {
      try {
        const opened = await terminalOpen(cwd, 80, 24);
        terminalsOpen.value = true;
        activeTerminal.value = opened.id;
        const cmd = providerId ? `omp auth-broker login ${providerId}\r` : "omp auth-broker login\r";
        await terminalWrite(opened.id, cmd);
      } catch (cause) {
        options.error.value = options.describe(cause);
      }
    })();
  }

  function openTerminalBeside(): void {
    const current = terminals.value.find((tab) => tab.id === activeTerminal.value)?.cwd;
    void openTerminal(current ?? options.workspace.value ?? "");
  }

  return {
    terminalsOpen,
    terminals,
    activeTerminal,
    init,
    openTerminal,
    selectTerminal,
    closeTerminal,
    logIn,
    openTerminalBeside,
  };
}
