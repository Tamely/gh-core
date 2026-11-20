// Detects the native iOS/Android shell without importing @capacitor/core.
// The shell defines window.Capacitor before our scripts run, so reading it is
// enough, and the web build never pays for Capacitor's JavaScript.

type CapacitorGlobal = {
  isNativePlatform?: () => boolean;
  getPlatform?: () => string;
};

function capacitor(): CapacitorGlobal | undefined {
  return (globalThis as { Capacitor?: CapacitorGlobal }).Capacitor;
}

export function isNative(): boolean {
  return capacitor()?.isNativePlatform?.() === true;
}

/** "ios", "android" or "web". */
export function platform(): string {
  return isNative() ? (capacitor()?.getPlatform?.() ?? "web") : "web";
}
