/** Command replies and change events can cross; the higher revision is the newer state. */
export function newestView<T extends { revision: number }>(current: T | undefined, next: T): T {
  return current && current.revision > next.revision ? current : next;
}
