import { describe, expect, test } from "bun:test";
import { ref } from "vue";
import type { RowSnapshot } from "../bridge";
import { useOptimisticSteer } from "./optimisticSteer";

function makeRow(partial: Partial<RowSnapshot>): RowSnapshot {
  return {
    role: "user",
    timestamp: Date.now(),
    text: "",
    thinking: null,
    streaming: false,
    tool: null,
    attachments: [],
    customType: null,
    jobs: [],
    command: null,
    ...partial,
  };
}

describe("useOptimisticSteer", () => {
  test("sets pending steer and triggers callback", () => {
    const rows = ref<RowSnapshot[]>([]);
    let callbackFired = false;
    const { pendingSteer, setSteering } = useOptimisticSteer(rows, () => {
      callbackFired = true;
    });

    expect(pendingSteer.value).toBeNull();
    setSteering({ text: "proceed with caution" });

    expect(pendingSteer.value?.text).toBe("proceed with caution");
    expect(callbackFired).toBe(true);
  });

  test("clears pending steer on empty text payload", () => {
    const rows = ref<RowSnapshot[]>([]);
    const { pendingSteer, setSteering } = useOptimisticSteer(rows);

    setSteering({ text: "do this" });
    expect(pendingSteer.value?.text).toBe("do this");

    setSteering({ text: "" });
    expect(pendingSteer.value).toBeNull();
  });

  test("clears pending steer when matching user row lands", async () => {
    const rows = ref<RowSnapshot[]>([
      makeRow({ role: "assistant", text: "working..." }),
    ]);
    const { pendingSteer, setSteering } = useOptimisticSteer(rows);

    setSteering({ text: "interrupt and stop" });
    expect(pendingSteer.value?.text).toBe("interrupt and stop");

    // Add intermediate tool rows and finally the user steer row
    rows.value = [
      ...rows.value,
      makeRow({ role: "tool", text: "done" }),
      makeRow({ role: "user", text: "interrupt and stop", steering: true }),
    ];

    await new Promise((r) => setTimeout(r, 10));
    expect(pendingSteer.value).toBeNull();
  });
});
