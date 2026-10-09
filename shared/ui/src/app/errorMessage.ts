/** The message of a thrown value; `fallback` stands in for a missing value. */
export function errorMessage(error: unknown, fallback?: string): string {
  if (error instanceof Error) return error.message;
  if (error == null && fallback !== undefined) return fallback;
  return String(error);
}
