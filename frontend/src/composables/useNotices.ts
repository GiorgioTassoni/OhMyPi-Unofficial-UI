import { onUnmounted, ref } from "vue";
import { addNotice, dropNotice, noticeForAction, type Notice } from "../lib/notify";

const NOTICE_LIMIT = 3;
const NOTICE_AUTO_DISMISS_DURATION = 6000;

/**
 * Manages the bottom-right in-app toast queue and its auto-dismiss timers.
 */
export function useNotices() {
  const notices = ref<Notice[]>([]);
  const timers = new Map<string, ReturnType<typeof setTimeout>>();
  let seq = 0;

  function clearTimer(id: string): void {
    const timer = timers.get(id);
    if (timer) {
      clearTimeout(timer);
      timers.delete(id);
    }
  }

  function scheduleAutoDismiss(id: string, durationMs = NOTICE_AUTO_DISMISS_DURATION): void {
    clearTimer(id);
    timers.set(
      id,
      setTimeout(() => {
        notices.value = dropNotice(notices.value, id);
        timers.delete(id);
      }, durationMs),
    );
  }

  function dismiss(id: string): void {
    clearTimer(id);
    notices.value = dropNotice(notices.value, id);
  }

  function push(notice: Notice): void {
    notices.value = addNotice(notices.value, notice, NOTICE_LIMIT);
    scheduleAutoDismiss(notice.id);
  }

  function nextId(): string {
    seq += 1;
    return `notice-${seq}`;
  }

  function showActionNotice(message: string): void {
    const id = nextId();
    push(noticeForAction(message, id));
  }

  function removeForThread(threadId: string): void {
    for (const entry of notices.value) {
      if (entry.thread === threadId) clearTimer(entry.id);
    }
    notices.value = notices.value.filter((entry) => entry.thread !== threadId);
  }

  onUnmounted(() => {
    for (const timer of timers.values()) {
      clearTimeout(timer);
    }
    timers.clear();
  });

  return {
    notices,
    nextId,
    push,
    dismiss,
    showActionNotice,
    removeForThread,
  };
}
