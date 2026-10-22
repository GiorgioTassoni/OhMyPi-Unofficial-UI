import type { NotificationKind } from "../bridge";

const STORAGE_KEY = "omp:notification-sound";

export type SoundKind = "finished" | "needs-you";

/** Only a completed turn or a question calls for a chime. */
export function soundForNotification(kind: NotificationKind): SoundKind | null {
  if (kind === "turn-finished") return "finished";
  if (kind === "needs-you") return "needs-you";
  return null;
}

/** Defaults to enabled (true) if the user has not explicitly disabled it in settings. */
export function readNotificationSoundEnabled(): boolean {
  try {
    const val = localStorage.getItem(STORAGE_KEY);
    return val === null ? true : val === "true";
  } catch {
    return true;
  }
}

export function saveNotificationSoundEnabled(enabled: boolean): void {
  try {
    localStorage.setItem(STORAGE_KEY, String(enabled));
  } catch {
    // The current window still honors the choice when storage is unavailable.
  }
}

let audio: AudioContext | null = null;

function getAudioContext(): AudioContext | null {
  if (typeof window === "undefined") return null;
  const Ctx =
    window.AudioContext ||
    (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!Ctx) return null;
  audio ??= new Ctx();
  return audio;
}

/**
 * Pre-warms and unlocks the AudioContext on the first user interaction (click/keypress).
 * Browsers and WebKitGTK enforce strict autoplay policies that suspend AudioContexts
 * unless resumed inside a user gesture; once unlocked, background notifications play freely.
 */
export function setupAudioUnlock(): () => void {
  if (typeof window === "undefined") return () => {};
  const unlock = () => {
    try {
      const ctx = getAudioContext();
      if (ctx && ctx.state === "suspended") {
        void ctx.resume();
      }
    } catch {
      // ignore
    }
  };
  window.addEventListener("pointerdown", unlock, { passive: true, capture: true });
  window.addEventListener("keydown", unlock, { passive: true, capture: true });
  return () => {
    window.removeEventListener("pointerdown", unlock, { capture: true });
    window.removeEventListener("keydown", unlock, { capture: true });
  };
}

function playWebAudioChime(ctx: AudioContext, kind: SoundKind): void {
  const notes = kind === "finished" ? [523.25, 659.25] : [659.25, 783.99];
  const start = ctx.currentTime + 0.05;
  for (const [index, frequency] of notes.entries()) {
    const at = start + index * 0.14;
    const oscillator = ctx.createOscillator();
    const envelope = ctx.createGain();
    oscillator.type = "sine";
    oscillator.frequency.setValueAtTime(frequency, at);

    envelope.gain.setValueAtTime(0.0001, at);
    envelope.gain.linearRampToValueAtTime(0.35, at + 0.025);
    envelope.gain.linearRampToValueAtTime(0.0001, at + 0.16);

    oscillator.connect(envelope);
    envelope.connect(ctx.destination);
    oscillator.start(at);
    oscillator.stop(at + 0.18);
    oscillator.onended = () => {
      oscillator.disconnect();
      envelope.disconnect();
    };
  }
}

/** Pure mathematical fallback synthesizing a short 16-bit PCM WAV for HTML5 Audio playback. */
function createChimeWav(kind: SoundKind): string {
  const sampleRate = 22050;
  const duration = 0.35;
  const numSamples = Math.floor(sampleRate * duration);
  const buffer = new ArrayBuffer(44 + numSamples * 2);
  const view = new DataView(buffer);

  // RIFF header
  view.setUint32(0, 0x52494646, false); // "RIFF"
  view.setUint32(4, 36 + numSamples * 2, true);
  view.setUint32(8, 0x57415645, false); // "WAVE"

  // fmt chunk
  view.setUint32(12, 0x666d7420, false); // "fmt "
  view.setUint16(16, 16, true);
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true); // byte rate
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);

  // data chunk
  view.setUint32(36, 0x64617461, false); // "data"
  view.setUint32(40, numSamples * 2, true);

  const notes = kind === "finished" ? [523.25, 659.25] : [659.25, 783.99];
  const half = Math.floor(numSamples / 2);

  for (let i = 0; i < numSamples; i++) {
    const isFirst = i < half;
    const freq = isFirst ? notes[0] : notes[1];
    const noteSample = isFirst ? i : i - half;
    const progress = noteSample / half;
    const env = progress < 0.15 ? progress / 0.15 : Math.max(0, 1 - (progress - 0.15) / 0.85);
    const sample = Math.sin(2 * Math.PI * freq * (i / sampleRate)) * env * 0.4;
    const intSample = Math.max(-32768, Math.min(32767, Math.floor(sample * 32767)));
    view.setInt16(44 + i * 2, intSample, true);
  }

  const bytes = new Uint8Array(buffer);
  let binary = "";
  for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
  return "data:audio/wav;base64," + btoa(binary);
}

const wavCache: Partial<Record<SoundKind, string>> = {};
function getWavUri(kind: SoundKind): string {
  return (wavCache[kind] ??= createChimeWav(kind));
}

function playWavFallback(kind: SoundKind): void {
  try {
    if (typeof Audio === "undefined") return;
    const audioEl = new Audio(getWavUri(kind));
    audioEl.volume = 0.5;
    void audioEl.play().catch(() => {});
  } catch {
    // ignore
  }
}

/** Play a two-note notification chime using Web Audio, with an HTML5 Audio fallback. */
export async function playNotificationSound(kind: SoundKind): Promise<void> {
  const ctx = getAudioContext();
  if (ctx) {
    if (ctx.state === "suspended") {
      try {
        await ctx.resume();
      } catch {
        // Fall back below if resume fails
      }
    }
    if (ctx.state === "running") {
      playWebAudioChime(ctx, kind);
      return;
    }
  }

  playWavFallback(kind);
}
