export type UsageWindowSnapshot = {
  usedPercent: number | null;
  resetAt: string | null;
  estimated: boolean;
};

type UsageProvider = {
  id: string;
  name: string;
  windows: Array<UsageWindowSnapshot & { windowMinutes: number }>;
};

export type UsageNotice = {
  kind: "limit" | "reset";
  providerName: string;
  windowMinutes: number;
  usedPercent: number | null;
};

export function usageTransitions(
  previous: ReadonlyMap<string, UsageWindowSnapshot>,
  providers: readonly UsageProvider[],
) {
  const snapshots = new Map<string, UsageWindowSnapshot>();
  const notices: UsageNotice[] = [];

  for (const provider of providers) {
    for (const window of provider.windows) {
      const key = `${provider.id}:${window.windowMinutes}`;
      const current = {
        usedPercent: window.usedPercent,
        resetAt: window.resetAt,
        estimated: window.estimated,
      };
      const before = previous.get(key);
      snapshots.set(key, current);
      if (!before || before.estimated !== current.estimated) continue;

      const resetChanged =
        before.resetAt !== null &&
        current.resetAt !== null &&
        before.resetAt !== current.resetAt;
      const usageDropped =
        before.usedPercent !== null &&
        current.usedPercent !== null &&
        before.usedPercent - current.usedPercent >= 5;
      const crossedLimit =
        before.usedPercent !== null &&
        current.usedPercent !== null &&
        before.usedPercent < 80 &&
        current.usedPercent >= 80;

      if (resetChanged || usageDropped) {
        notices.push({
          kind: "reset",
          providerName: provider.name,
          windowMinutes: window.windowMinutes,
          usedPercent: current.usedPercent,
        });
      } else if (crossedLimit) {
        notices.push({
          kind: "limit",
          providerName: provider.name,
          windowMinutes: window.windowMinutes,
          usedPercent: current.usedPercent,
        });
      }
    }
  }

  return { snapshots, notices };
}
