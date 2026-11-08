import { useCallback, useEffect, useState } from "react";

import { copyText } from "./clipboard";

export type CopyResult = "copied" | "failed" | null;

/**
 * Copies text and says how it went for a moment: `copied` for `shownMs`,
 * `failed` a second longer, then nothing again.
 */
export function useCopyFeedback(shownMs = 1_500): { result: CopyResult; copy: (text: string) => void } {
  const [result, setResult] = useState<CopyResult>(null);
  useEffect(() => {
    if (!result) return undefined;
    const timer = window.setTimeout(() => setResult(null), result === "copied" ? shownMs : shownMs + 1_000);
    return () => window.clearTimeout(timer);
  }, [result, shownMs]);
  const copy = useCallback((text: string) => {
    void copyText(text).then(() => setResult("copied"), () => setResult("failed"));
  }, []);
  return { result, copy };
}
