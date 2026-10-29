const FRAME_WAIT_MS = 1_000;
const CHECK_PAUSE_MS = 2_000;
const RECOVERY_PAUSE_MS = 10_000;
const INPUT_EVENTS = ["pointerdown", "pointermove", "keydown", "wheel", "focus", "resize"] as const;

/**
 * WebKitGTK can stop drawing a visible page after its window is minimized or
 * resized: script keeps running but no frame is painted again. Someone using
 * the window while two frame requests in a row go unanswered means the page is
 * stuck, and the shell is asked to bring it back. Only input starts a check,
 * because a minimized window may still report itself visible on Wayland.
 */
export function installRenderWatchdog(
  recover: () => void | Promise<void>,
  target: Window = window,
): () => void {
  let checking = false;
  let lastCheck = Number.NEGATIVE_INFINITY;
  let lastRecovery = Number.NEGATIVE_INFINITY;
  const visible = () => target.document.visibilityState === "visible";

  const frameArrives = () => new Promise<boolean>((resolve) => {
    let drawn = false;
    const frame = target.requestAnimationFrame(() => {
      drawn = true;
    });
    target.setTimeout(() => {
      if (!drawn) target.cancelAnimationFrame(frame);
      resolve(drawn);
    }, FRAME_WAIT_MS);
  });

  const check = async () => {
    const startedAt = target.performance.now();
    if (checking || startedAt - lastCheck < CHECK_PAUSE_MS || !visible()) return;
    checking = true;
    lastCheck = startedAt;
    try {
      // A busy page can miss one frame; a stuck one misses both.
      if (await frameArrives() || !visible() || await frameArrives() || !visible()) return;
      const now = target.performance.now();
      if (now - lastRecovery < RECOVERY_PAUSE_MS) return;
      lastRecovery = now;
      await recover();
    } catch {
      // Recovery is best effort; the next input checks again.
    } finally {
      checking = false;
    }
  };

  const listener = () => {
    void check();
  };
  for (const event of INPUT_EVENTS) {
    target.addEventListener(event, listener, { capture: true, passive: true });
  }
  return () => {
    for (const event of INPUT_EVENTS) {
      target.removeEventListener(event, listener, { capture: true });
    }
  };
}
