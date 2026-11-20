// Records how long each launch took to render the shell, for the offline
// startup target: under 100ms from performance.timeOrigin to "shell:rendered",
// median of 10 cold starts in Airplane Mode (handoffs/josh.md, docs/native.md).
//
// Each launch's time is kept on the device, so you can do all 10 cold starts
// and then read the results once from Safari Web Inspector or chrome://inspect.

const KEY = "gh:startup:v1";
const KEEP = 20;

export function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2
    ? (sorted[mid] as number)
    : ((sorted[mid - 1] as number) + (sorted[mid] as number)) / 2;
}

export function recordStartup(platform: string): void {
  const mark = performance.getEntriesByName("shell:rendered")[0];
  if (!mark) return;
  const ms = Math.round(mark.startTime);
  try {
    const previous = JSON.parse(localStorage.getItem(KEY) ?? "[]") as number[];
    const runs = [...previous, ms].slice(-KEEP);
    localStorage.setItem(KEY, JSON.stringify(runs));
    console.info(
      `[startup] ${platform}: shell rendered in ${ms}ms. Last ${runs.length} launches: ${runs.join(", ")}. Median ${median(runs)}ms.`,
    );
  } catch {
    console.info(`[startup] ${platform}: shell rendered in ${ms}ms.`);
  }
}

/** Clears the recorded launches, before starting a new round of 10. */
export function clearStartups(): void {
  try {
    localStorage.removeItem(KEY);
  } catch {
    // Nothing recorded.
  }
}
