<script setup lang="ts">
/**
 * Read-only file viewer for the right sidebar.
 *
 * Opens when an agent reads a whole file (or when inspected), presenting:
 * - Image files (png, jpg, webp, svg, gif, etc.) with zoom controls (fit vs 100%),
 *   resolution metrics, and a subtle checkerboard transparency canvas.
 * - Markdown documents via MarkdownBody with Preview / Raw segmented toggle.
 * - Code and text files with a lockstep line-numbered viewer, safe syntax highlighting,
 *   copy affordances, and an external editor launch button.
 *
 * Self-contained: fetches its own content from artifacts or disk when not pre-supplied.
 */
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { openPath, readArtifact, readFileBase64, readFileContent } from "../bridge";
import {
  formatLanguageLabel,
  isImageExtension,
  isMarkdownExtension,
  isSvgExtension,
  mimeTypeForExtension,
  parseFilePath,
} from "../lib/fileViewMeta";
import { highlightCode } from "../lib/highlighter";
import Icon from "./ui/Icon.vue";
import MarkdownBody from "./MarkdownBody.vue";

export interface FileViewRequest {
  path: string;
  content?: string;
  artifactId?: string | null;
}

const props = defineProps<{
  file: FileViewRequest;
  thread: string;
  workspace?: string | null | undefined;
}>();

const emit = defineEmits<{
  (e: "close"): void;
  (e: "failed", message: string): void;
}>();

const content = ref(props.file.content ?? "");
const loading = ref(true);
const error = ref<string | null>(null);

const imageDataUrl = ref<string | null>(null);
const imageDimensions = ref<{ width: number; height: number } | null>(null);
const imageZoom = ref<"fit" | "actual">("fit");

const meta = computed(() => parseFilePath(props.file.path));
const cleanPath = computed(() => meta.value.cleanPath);
const lineRange = computed(() => meta.value.lineRange);
const basename = computed(() => meta.value.basename);
const directory = computed(() => meta.value.directory);
const extension = computed(() => meta.value.extension);

const isImage = computed(() => isImageExtension(extension.value));
const isSvg = computed(() => isSvgExtension(extension.value));
const isMarkdown = computed(() => isMarkdownExtension(extension.value));

/** Segmented view toggle for markdown & SVG documents: formatted Preview vs raw line-numbered source. */
const mode = ref<"preview" | "raw">(isMarkdown.value || isSvg.value ? "preview" : "raw");

watch(extension, () => {
  if (isMarkdown.value || isSvg.value) {
    mode.value = "preview";
  } else {
    mode.value = "raw";
  }
});

async function load(): Promise<void> {
  loading.value = true;
  error.value = null;
  imageDimensions.value = null;
  imageDataUrl.value = null;

  try {
    if (isImage.value) {
      // Binary images must always be read from disk as base64 (tool.output is summary text, not image bytes)
      const b64 = await readFileBase64(props.workspace ?? null, cleanPath.value);
      const mime = mimeTypeForExtension(extension.value);
      imageDataUrl.value = `data:${mime};base64,${b64}`;
      if (isSvg.value) {
        try {
          content.value = atob(b64);
        } catch {
          content.value = "";
        }
      }
      return;
    }

    if (props.file.content) {
      content.value = props.file.content;
      return;
    }

    if (props.file.artifactId) {
      const art = await readArtifact(props.thread, props.file.artifactId);
      content.value = art.text;
    } else {
      const text = await readFileContent(props.workspace ?? null, cleanPath.value);
      content.value = text;
    }
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    loading.value = false;
  }
}

watch(() => [props.file.path, props.file.content, props.file.artifactId], load, { immediate: true });

function onImageLoad(e: Event): void {
  const target = e.target as HTMLImageElement;
  if (target) {
    imageDimensions.value = {
      width: target.naturalWidth,
      height: target.naturalHeight,
    };
  }
}

const lines = computed(() => {
  if (!content.value) return [""];
  return content.value.split(/\r?\n/);
});

const copied = ref(false);
let copyTimer: ReturnType<typeof setTimeout> | null = null;

async function copyContent(): Promise<void> {
  if (!content.value) return;
  try {
    await navigator.clipboard.writeText(content.value);
    markCopied();
  } catch (err) {
    emit("failed", `Failed to copy file: ${String(err)}`);
  }
}

async function copyImage(): Promise<void> {
  if (!imageDataUrl.value) return;
  try {
    const res = await fetch(imageDataUrl.value);
    const blob = await res.blob();
    if (blob.type === "image/png" && window.ClipboardItem) {
      await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
      markCopied();
      return;
    }

    // Convert to PNG on canvas for standard clipboard support
    const img = new Image();
    img.crossOrigin = "anonymous";
    await new Promise((resolve, reject) => {
      img.onload = resolve;
      img.onerror = reject;
      img.src = imageDataUrl.value!;
    });
    const canvas = document.createElement("canvas");
    canvas.width = img.naturalWidth;
    canvas.height = img.naturalHeight;
    const ctx = canvas.getContext("2d");
    if (ctx) {
      ctx.drawImage(img, 0, 0);
      const pngBlob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
      if (pngBlob && window.ClipboardItem) {
        await navigator.clipboard.write([new ClipboardItem({ "image/png": pngBlob })]);
        markCopied();
        return;
      }
    }
    // Fallback: copy file path
    await navigator.clipboard.writeText(props.file.path);
    markCopied();
  } catch {
    try {
      await navigator.clipboard.writeText(props.file.path);
      markCopied();
    } catch (err) {
      emit("failed", `Failed to copy image: ${String(err)}`);
    }
  }
}

function markCopied(): void {
  copied.value = true;
  if (copyTimer) clearTimeout(copyTimer);
  copyTimer = setTimeout(() => {
    copied.value = false;
  }, 1800);
}

async function openInEditor(): Promise<void> {
  const targetPath = cleanPath.value;
  const fullPath = props.workspace && !targetPath.startsWith("/")
    ? `${props.workspace}/${targetPath}`
    : targetPath;
  try {
    await openPath(fullPath);
  } catch (err) {
    emit("failed", `Could not open file in editor: ${String(err)}`);
  }
}

function onKeydown(e: KeyboardEvent): void {
  if (e.key === "Escape") {
    e.preventDefault();
    emit("close");
  }
}

onMounted(() => {
  window.addEventListener("keydown", onKeydown);
});

onUnmounted(() => {
  window.removeEventListener("keydown", onKeydown);
  if (copyTimer) clearTimeout(copyTimer);
});

const languageLabel = computed(() => formatLanguageLabel(extension.value));

const formattedLines = computed(() => highlightCode(content.value, extension.value));
</script>

<template>
  <div class="flex h-full w-full min-w-0 flex-col bg-canvas text-fg" role="region" aria-label="Read-only file viewer">
    <!-- Top Bar -->
    <header class="flex h-10 shrink-0 items-center justify-between border-b border-line/50 bg-surface/60 px-2.5 backdrop-blur-sm select-none">
      <div class="flex items-center gap-2 min-w-0 flex-1 pr-2">
        <!-- Back Button -->
        <button
          type="button"
          class="flex items-center gap-1.5 rounded-[5px] px-2 py-1 text-[11.5px] font-medium text-dim transition-colors hover:bg-raised hover:text-fg"
          aria-label="Back to panel"
          @click="emit('close')"
        >
          <Icon name="arrow-left" class="h-3.5 w-3.5 shrink-0" />
          <span class="hidden sm:inline">Back</span>
        </button>

        <span class="h-3.5 w-px bg-line/60" />

        <!-- File Identity -->
        <div class="flex items-center gap-1.5 min-w-0 truncate">
          <Icon :name="isImage ? 'image' : 'file'" class="h-3.5 w-3.5 shrink-0 text-accent" />
          <span class="truncate font-mono text-[12.5px] font-semibold text-fg">
            {{ basename }}
          </span>
          <span v-if="lineRange" class="shrink-0 rounded bg-raised px-1 py-0.5 font-mono text-[10.5px] text-accent font-medium">
            :{{ lineRange }}
          </span>
          <span v-if="directory" class="hidden md:inline truncate font-mono text-[11px] text-faint">
            {{ directory }}
          </span>
        </div>
      </div>

      <!-- Actions & Controls -->
      <div class="flex items-center gap-2 shrink-0">
        <!-- Markdown & SVG Preview vs Raw Toggle -->
        <div v-if="isMarkdown || isSvg" class="flex items-center rounded-[5px] border border-line/60 bg-raised/50 p-0.5 text-[11px]">
          <button
            type="button"
            class="rounded-[4px] px-2 py-0.5 font-medium transition-colors"
            :class="mode === 'preview' ? 'bg-canvas text-fg shadow-xs' : 'text-dim hover:text-fg'"
            @click="mode = 'preview'"
          >
            Preview
          </button>
          <button
            type="button"
            class="rounded-[4px] px-2 py-0.5 font-medium transition-colors"
            :class="mode === 'raw' ? 'bg-canvas text-fg shadow-xs' : 'text-dim hover:text-fg'"
            @click="mode = 'raw'"
          >
            Raw
          </button>
        </div>

        <!-- Image Zoom Toggle (Fit vs 100%) -->
        <div v-else-if="isImage" class="flex items-center rounded-[5px] border border-line/60 bg-raised/50 p-0.5 text-[11px]">
          <button
            type="button"
            class="rounded-[4px] px-2 py-0.5 font-medium transition-colors"
            :class="imageZoom === 'fit' ? 'bg-canvas text-fg shadow-xs' : 'text-dim hover:text-fg'"
            title="Fit image inside view"
            @click="imageZoom = 'fit'"
          >
            Fit
          </button>
          <button
            type="button"
            class="rounded-[4px] px-2 py-0.5 font-medium transition-colors"
            :class="imageZoom === 'actual' ? 'bg-canvas text-fg shadow-xs' : 'text-dim hover:text-fg'"
            title="View at 100% natural size"
            @click="imageZoom = 'actual'"
          >
            100%
          </button>
        </div>

        <!-- Language / Format & Dimension / Line Count Badges -->
        <span class="rounded bg-raised px-1.5 py-0.5 font-mono text-[10.5px] text-dim">
          {{ languageLabel }}
        </span>
        <span v-if="isImage && imageDimensions" class="hidden sm:inline font-mono text-[11px] text-faint">
          {{ imageDimensions.width }} × {{ imageDimensions.height }}
        </span>
        <span v-else-if="!isImage && !loading && !error" class="hidden sm:inline font-mono text-[11px] text-faint">
          {{ lines.length }} {{ lines.length === 1 ? 'line' : 'lines' }}
        </span>

        <span class="h-3.5 w-px bg-line/60" />

        <!-- Copy File / Image -->
        <button
          type="button"
          class="flex items-center gap-1 rounded-[5px] border border-line/50 bg-raised/40 px-2 py-1 text-[11.5px] text-dim transition-colors hover:border-line-strong hover:bg-raised hover:text-fg"
          :aria-label="isImage && (!isSvg || mode === 'preview') ? 'Copy image' : 'Copy file contents'"
          :disabled="loading || Boolean(error)"
          @click="isImage && (!isSvg || mode === 'preview') ? copyImage() : copyContent()"
        >
          <Icon :name="copied ? 'check' : 'copy'" class="h-3.5 w-3.5 shrink-0" :class="copied ? 'text-ok' : ''" />
          <span :class="copied ? 'text-ok font-medium' : ''">{{ copied ? 'Copied' : 'Copy' }}</span>
        </button>

        <!-- Open in External Editor -->
        <button
          type="button"
          class="rounded-[5px] p-1 text-dim transition-colors hover:bg-raised hover:text-fg"
          aria-label="Open in native editor"
          @click="openInEditor"
        >
          <Icon name="external" class="h-3.5 w-3.5" />
        </button>

        <!-- Close Button -->
        <button
          type="button"
          class="rounded-[5px] p-1 text-faint transition-colors hover:bg-raised hover:text-fg"
          aria-label="Close file viewer"
          @click="emit('close')"
        >
          <Icon name="close" class="h-3.5 w-3.5" />
        </button>
      </div>
    </header>

    <!-- Subtitle metadata strip -->
    <div class="flex items-center justify-between border-b border-line/30 bg-surface/30 px-3.5 py-1 text-[11px] text-faint select-none">
      <div class="flex items-center gap-2 truncate">
        <span class="flex items-center gap-1 text-accent/90 font-medium">
          <span class="h-1.5 w-1.5 rounded-full bg-accent" />
          {{ isImage && (!isSvg || mode === 'preview') ? 'Image preview' : 'Read-only view' }}
        </span>
        <span>·</span>
        <span class="truncate font-mono text-faint/80">{{ props.file.path }}</span>
      </div>
      <div class="flex items-center gap-2 shrink-0 font-mono text-[10.5px] text-faint/70">
        <span v-if="isImage && (!isSvg || mode === 'preview')" class="hidden sm:inline">click image to zoom ·</span>
        <span class="hidden sm:inline">esc to close</span>
      </div>
    </div>

    <!-- Main Content Area -->
    <main class="min-h-0 flex-1 overflow-hidden relative">
      <!-- Loading State -->
      <div v-if="loading" class="flex h-full w-full items-center justify-center gap-2.5 text-[12.5px] text-dim">
        <Icon name="refresh" class="h-4 w-4 animate-spin text-accent" />
        <span>Loading file content…</span>
      </div>

      <!-- Error State -->
      <div v-else-if="error" class="flex h-full w-full flex-col items-center justify-center p-6 text-center">
        <div class="rounded-full bg-err/10 p-3 text-err mb-2">
          <Icon name="close" class="h-5 w-5" />
        </div>
        <h4 class="text-[13px] font-semibold text-fg">Unable to load file</h4>
        <p class="mt-1 max-w-sm text-[12px] text-dim whitespace-pre-wrap">{{ error }}</p>
        <button
          type="button"
          class="mt-3 rounded-[6px] border border-line bg-raised px-3 py-1.5 text-[12px] font-medium text-fg hover:bg-raised/80"
          @click="openInEditor"
        >
          Open in native editor instead
        </button>
      </div>

      <!-- Image View (Raster images or SVG Preview) -->
      <div
        v-else-if="isImage && (!isSvg || mode === 'preview')"
        class="checkerboard-pattern relative flex h-full w-full items-center justify-center overflow-auto p-4 select-none"
      >
        <div
          v-if="imageDataUrl"
          class="flex items-center justify-center transition-all duration-150"
          :class="imageZoom === 'fit' ? 'h-full w-full' : 'min-h-full min-w-full p-4'"
        >
          <img
            :src="imageDataUrl"
            :alt="basename"
            class="rounded-[4px] shadow-sm ring-1 ring-line/40 transition-transform duration-100 select-none"
            :class="[
              imageZoom === 'fit' ? 'max-h-full max-w-full object-contain cursor-zoom-in' : 'cursor-zoom-out',
            ]"
            @load="onImageLoad"
            @click="imageZoom = imageZoom === 'fit' ? 'actual' : 'fit'"
          />
        </div>
      </div>

      <!-- Markdown Preview View -->
      <div
        v-else-if="isMarkdown && mode === 'preview'"
        class="h-full w-full overflow-auto p-5 select-text leading-relaxed"
      >
        <MarkdownBody :text="content" @failed="emit('failed', $event)" />
      </div>

      <!-- Code & Raw Text Lockstep Line-Numbered View -->
      <div
        v-else
        class="h-full w-full overflow-auto font-mono text-[12px] leading-[1.65] select-text py-2"
      >
        <div
          v-for="(line, idx) in formattedLines"
          :key="idx"
          class="flex items-start hover:bg-raised/35 px-2 transition-colors group/codeline"
        >
          <!-- Lockstep Line Number -->
          <span
            class="w-12 shrink-0 select-none text-right pr-4 text-faint/50 font-mono text-[11px] group-hover/codeline:text-dim"
          >
            {{ idx + 1 }}
          </span>
          <!-- Highlighted Line Content -->
          <span
            class="min-w-0 flex-1 whitespace-pre font-mono text-[12px] text-fg"
            v-html="line"
          />
        </div>
      </div>
    </main>
  </div>
</template>

<style scoped>
.checkerboard-pattern {
  background-color: var(--color-canvas, #101216);
  background-image:
    linear-gradient(45deg, rgba(128, 128, 128, 0.07) 25%, transparent 25%),
    linear-gradient(-45deg, rgba(128, 128, 128, 0.07) 25%, transparent 25%),
    linear-gradient(45deg, transparent 75%, rgba(128, 128, 128, 0.07) 75%),
    linear-gradient(-45deg, transparent 75%, rgba(128, 128, 128, 0.07) 75%);
  background-size: 16px 16px;
  background-position: 0 0, 0 8px, 8px -8px, -8px 0px;
}
</style>
