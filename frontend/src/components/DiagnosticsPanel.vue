<script setup lang="ts">
/**
 * The M0 readouts, kept rather than deleted.
 *
 * `docs/14` §2's M0 milestone is "the window prints the ready frame fields", and those
 * numbers are still the fastest way to tell a handshake failure from a hung turn: the
 * negotiated protocol version, the frame ceiling, the event counters, and the raw event
 * tail. `docs/12` §1 has no debug pane in the product window, so this lives behind a
 * toggle instead of taking a column.
 */
import type { ActivitySnapshot, CounterSnapshot, ReadySnapshot, SessionStatus } from "../bridge";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  thread: string | null;
  ready: ReadySnapshot | null;
  status: SessionStatus | null;
  counters: CounterSnapshot | null;
  activity: ActivitySnapshot[];
  /** The agent is waiting on a dialog, so the run is parked. */
  blocked: boolean;
  workspace?: string | null;
  terminals?: boolean;
}>();

const emit = defineEmits<{
  close: [];
  toggleTerminals: [];
  togglePanel: [];
}>();

/** The tail is capped: this pane shows liveness, not history. */
const ACTIVITY_TAIL = 60;

/**
 * The panel is one readout per line: the engine's own name on the left, its value on the right.
 *
 * The value cell carries no colour of its own — a counter that has gone wrong is tinted and the
 * rest are `text-dim` — so a row cannot end up with two colour utilities fighting over which
 * one the stylesheet happens to place last.
 */
const ROW = "flex items-start justify-between gap-4 px-2 py-1.5";
const KEY = "text-[11.5px] text-faint";
const VALUE = "break-all font-mono text-[11.5px]";

const BUTTON =
  "grid h-7 w-7 place-items-center rounded-[6px] text-dim transition-colors hover:bg-raised hover:text-fg disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-dim";
</script>

<template>
  <aside class="flex w-full h-full shrink-0 flex-col border-l border-line/40 bg-canvas" aria-label="diagnostics panel">
    <!-- Top bar matches TitleBar and Sidebar height (h-10 / 40px) -->
    <header
      class="flex h-10 shrink-0 items-center justify-between border-b border-line/40 px-2.5 select-none"
      data-tauri-drag-region
    >
      <span class="text-[12.5px] font-semibold tracking-tight text-fg/80 pl-1">Diagnostics</span>
      <div class="ml-auto flex items-center gap-0.5 shrink-0">
        <button
          :class="[BUTTON, props.terminals ? 'bg-raised text-accent hover:text-accent' : '']"
          :disabled="props.workspace === null"
          title="the terminal panel: your own shell in this workspace, not the agent's"
          aria-label="terminal"
          data-action="terminal"
          type="button"
          @click="emit('toggleTerminals')"
        >
          <Icon name="terminal" />
        </button>
        <button
          :class="BUTTON"
          title="the thread panel: its plan and the files it changed"
          aria-label="thread panel"
          data-action="panel"
          type="button"
          @click="emit('togglePanel')"
        >
          <Icon name="panels" />
        </button>
        <button
          :class="[BUTTON, 'bg-raised text-accent hover:text-accent']"
          title="the M0 readouts: handshake, process, stream counters, event tail"
          aria-label="diagnostics"
          data-action="diagnostics"
          type="button"
          @click="emit('close')"
        >
          <Icon name="sliders" />
        </button>
      </div>
    </header>

    <div class="flex flex-1 flex-col gap-4 overflow-auto px-3.5 py-3">
      <section class="flex flex-col">
        <h2 class="px-1 pb-0.5 text-[10.5px] font-medium uppercase tracking-[0.09em] text-faint">
          ready
        </h2>
      <dl v-if="props.ready" class="flex flex-col">
        <div :class="ROW">
          <dt :class="KEY">protocol</dt>
          <dd :class="[VALUE, 'text-dim']">v{{ props.ready.protocolVersion }}</dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">supports</dt>
          <dd :class="[VALUE, 'text-dim']">{{ props.ready.supportedProtocolVersions.join(", ") }}</dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">negotiated v2</dt>
          <dd
            class="text-[11.5px]"
            :class="props.ready.negotiatedV2 ? 'text-ok' : 'text-err'"
          >
            {{ props.ready.negotiatedV2 ? "yes" : "NO — catalogue undeliverable" }}
          </dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">maxFrame</dt>
          <dd :class="[VALUE, 'text-dim']">{{ props.ready.maxFrameBytes.toLocaleString() }} B</dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">maxReassembled</dt>
          <dd :class="[VALUE, 'text-dim']">
            {{ props.ready.maxReassembledFrameBytes.toLocaleString() }} B
          </dd>
        </div>
      </dl>
      <p v-else class="px-2 py-1.5 text-[11.5px] text-faint">not connected</p>
    </section>

    <section class="flex flex-col">
      <h2 class="px-1 pb-0.5 text-[10.5px] font-medium uppercase tracking-[0.09em] text-faint">
        process
      </h2>
      <!--
        The thread is named outside the status guard on purpose: which thread the drawer is
        describing is worth knowing precisely when there is no status to read.
      -->
      <dl class="flex flex-col">
        <div :class="ROW">
          <dt :class="KEY">thread</dt>
          <dd :class="[VALUE, 'text-dim']">{{ props.thread ?? "—" }}</dd>
        </div>
        <template v-if="props.status">
          <div :class="ROW">
            <dt :class="KEY">pid</dt>
            <dd :class="[VALUE, 'text-dim']">{{ props.status.sidecarPid ?? "—" }}</dd>
          </div>
          <div :class="ROW">
            <dt :class="KEY">binary</dt>
            <dd :class="[VALUE, 'text-dim']">{{ props.status.binary }}</dd>
          </div>
          <div :class="ROW">
            <dt :class="KEY">workspace</dt>
            <dd :class="[VALUE, 'text-dim']">{{ props.status.workspace }}</dd>
          </div>
        </template>
      </dl>
      <p v-if="!props.status" class="px-2 py-1.5 text-[11.5px] text-faint">—</p>
    </section>

    <section class="flex flex-col">
      <h2 class="px-1 pb-0.5 text-[10.5px] font-medium uppercase tracking-[0.09em] text-faint">
        stream
      </h2>
      <dl v-if="props.counters" class="flex flex-col">
        <div :class="ROW">
          <dt :class="KEY">events</dt>
          <dd :class="[VALUE, 'text-dim']">{{ props.counters.eventsSeen }}</dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">kinds</dt>
          <dd :class="[VALUE, 'text-dim']">{{ props.counters.eventKinds }}</dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">frames</dt>
          <dd :class="[VALUE, 'text-dim']">{{ props.counters.framesSeen }}</dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">unknown</dt>
          <dd :class="[VALUE, props.counters.unknownEvents > 0 ? 'text-warn' : 'text-dim']">
            {{ props.counters.unknownEvents }}
          </dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">malformed</dt>
          <dd :class="[VALUE, props.counters.malformedEvents > 0 ? 'text-err' : 'text-dim']">
            {{ props.counters.malformedEvents }}
          </dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">lagged</dt>
          <dd :class="[VALUE, props.counters.laggedEvents > 0 ? 'text-warn' : 'text-dim']">
            {{ props.counters.laggedEvents }}
          </dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">ui requests</dt>
          <dd :class="[VALUE, 'text-dim']">{{ props.counters.uiRequests }}</dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">blocking seen</dt>
          <dd :class="[VALUE, props.counters.blockingUiRequests > 0 ? 'text-warn' : 'text-dim']">
            {{ props.counters.blockingUiRequests }}
          </dd>
        </div>
      </dl>
      <p v-else class="px-2 py-1.5 text-[11.5px] text-faint">—</p>
      <p v-if="props.blocked" class="mt-1 px-2 text-[11.5px] leading-relaxed text-warn">
        the agent is waiting on a dialog — answer it in the thread, or it will not continue.
      </p>
    </section>

    <section class="flex flex-col">
      <h2 class="px-1 pb-0.5 text-[10.5px] font-medium uppercase tracking-[0.09em] text-faint">
        session state
      </h2>
      <dl v-if="props.status" class="flex flex-col">
        <div :class="ROW">
          <dt :class="KEY">session</dt>
          <dd :class="[VALUE, 'text-dim']">{{ props.status.control.sessionId }}</dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">name</dt>
          <dd class="text-[11.5px] text-dim">{{ props.status.control.sessionName ?? "—" }}</dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">model</dt>
          <dd :class="[VALUE, 'text-dim']">
            {{ props.status.control.model ? `${props.status.control.model.provider}/${props.status.control.model.id}` : "—" }}
          </dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">thinking</dt>
          <dd class="text-[11.5px] text-dim">{{ props.status.control.thinkingLevel ?? "—" }}</dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">streaming</dt>
          <dd
            class="text-[11.5px]"
            :class="props.status.control.isStreaming ? 'text-ok' : 'text-dim'"
          >
            {{ props.status.control.isStreaming }}
          </dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">messages</dt>
          <dd :class="[VALUE, 'text-dim']">{{ props.status.control.messageCount }}</dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">queued</dt>
          <dd :class="[VALUE, 'text-dim']">{{ props.status.control.queuedMessageCount }}</dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">rows</dt>
          <dd :class="[VALUE, 'text-dim']">{{ props.status.control.transcriptRows }}</dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">todos</dt>
          <dd :class="[VALUE, 'text-dim']">{{ props.status.control.todoPhases.length }}</dd>
        </div>
        <div :class="ROW">
          <dt :class="KEY">context</dt>
          <dd v-if="props.status.control.context" :class="[VALUE, 'text-dim']">
            {{ props.status.control.context.tokens.toLocaleString() }} /
            {{ props.status.control.context.contextWindow.toLocaleString() }}
            ({{ props.status.control.context.percent.toFixed(1) }}%)
          </dd>
          <dd v-else :class="[VALUE, 'text-dim']">—</dd>
        </div>
      </dl>
      <p v-else class="px-2 py-1.5 text-[11.5px] text-faint">—</p>
    </section>

    <section class="flex flex-col">
      <h2 class="px-1 pb-0.5 text-[10.5px] font-medium uppercase tracking-[0.09em] text-faint">
        event tail
      </h2>
      <ul class="max-h-40 overflow-auto px-1 py-0.5 font-mono text-[11px] text-dim">
        <li
          v-for="event in props.activity.slice(-ACTIVITY_TAIL)"
          :key="event.sequence"
          class="flex gap-2 px-1 py-0.5"
        >
          <span class="shrink-0 text-faint">{{ event.sequence }}</span>
          <span class="truncate">{{ event.kind }}</span>
        </li>
        <li v-if="props.activity.length === 0" class="px-1 py-0.5 text-faint">idle</li>
      </ul>
    </section>
    </div>
  </aside>
</template>
