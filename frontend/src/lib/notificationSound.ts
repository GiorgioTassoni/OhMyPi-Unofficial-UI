import type { NotificationKind } from "../bridge";

const STORAGE_KEY = "omp:notification-sound";

export type SoundKind = "finished" | "needs-you";

/** Only a completed turn or a question calls for a chime. */
export function soundForNotification(kind: NotificationKind): SoundKind | null {
  if (kind === "turn-finished") return "finished";
  if (kind === "needs-you") return "needs-you";
  return null;
}

export function readNotificationSoundEnabled(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === "true";
  } catch {
    return false;
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

/** A soft two-note chime, synthesized locally with no downloaded sound or OS sound theme. */
export async function playNotificationSound(kind: SoundKind): Promise<void> {
  if (typeof AudioContext === "undefined") throw new Error("Audio playback is unavailable in this webview.");
  audio ??= new AudioContext();
  if (audio.state === "suspended") await audio.resume();
  if (audio.state !== "running") throw new Error("Audio playback is blocked until the app receives an interaction.");

  const notes = kind === "finished" ? [523.25, 659.25] : [659.25, 783.99];
  const start = audio.currentTime + 0.01;
  for (const [index, frequency] of notes.entries()) {
    const at = start + index * 0.14;
    const oscillator = audio.createOscillator();
    const envelope = audio.createGain();
    oscillator.type = "sine";
    oscillator.frequency.value = frequency;
    envelope.gain.setValueAtTime(0.0001, at);
    envelope.gain.exponentialRampToValueAtTime(0.2, at + 0.018);
    envelope.gain.exponentialRampToValueAtTime(0.0001, at + 0.16);
    oscillator.connect(envelope);
    envelope.connect(audio.destination);
    oscillator.start(at);
    oscillator.stop(at + 0.17);
    oscillator.onended = () => {
      oscillator.disconnect();
      envelope.disconnect();
    };
  }
}
