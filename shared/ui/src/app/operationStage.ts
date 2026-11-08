/** A long-running operation's stage once it can change no more. */
export function operationIsTerminal(stage: string | undefined): boolean {
  return stage === "completed" || stage === "cancelled" || stage === "failed";
}
