// Native haptics. On the web these do nothing; the plugin is only imported
// inside the native shell, so it never reaches the web bundle.
import { isNative } from "./platform";

type Haptics = typeof import("@capacitor/haptics");
let plugin: Promise<Haptics> | undefined;
const load = () => (plugin ??= import("@capacitor/haptics"));

/** A light tap, for pressing tabs and buttons. */
export function tapFeedback(): void {
  if (!isNative()) return;
  void load().then(({ Haptics, ImpactStyle }) =>
    Haptics.impact({ style: ImpactStyle.Light }),
  );
}

/** Success or error, for scan results (used by the scanner in PR 1.5). */
export function resultFeedback(ok: boolean): void {
  if (!isNative()) return;
  void load().then(({ Haptics, NotificationType }) =>
    Haptics.notification({
      type: ok ? NotificationType.Success : NotificationType.Error,
    }),
  );
}
