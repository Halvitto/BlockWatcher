import assert from "node:assert/strict";
import test from "node:test";
import { usageTransitions } from "../src/notifications.ts";

const provider = (usedPercent: number, resetAt: string | null = null) => [
  {
    id: "claude",
    name: "Claude",
    windows: [
      { usedPercent, resetAt, estimated: false, windowMinutes: 300 },
    ],
  },
];

test("detects limit crossings and resets without alerting on first load", () => {
  const initial = usageTransitions(new Map(), provider(79));
  assert.deepEqual(initial.notices, []);

  const limit = usageTransitions(initial.snapshots, provider(81));
  assert.equal(limit.notices[0]?.kind, "limit");

  const reset = usageTransitions(limit.snapshots, provider(12));
  assert.equal(reset.notices[0]?.kind, "reset");

  const scheduled = usageTransitions(
    usageTransitions(
      new Map(),
      provider(40, "2026-07-18T12:00:00Z"),
    ).snapshots,
    provider(40, "2026-07-18T17:00:00Z"),
  );
  assert.equal(scheduled.notices[0]?.kind, "reset");
});
