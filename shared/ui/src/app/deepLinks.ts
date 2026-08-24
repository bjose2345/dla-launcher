const READ_ONLY_WORK_LINK = /^dla-launcher:\/\/works\/((?:RJ|BJ|VJ)\d{5,10})$/i;

export interface ReadOnlyDeepLinkTarget {
  kind: "work";
  code: string;
}

export interface ReadOnlyDeepLinkGateway {
  readCurrent(): Promise<readonly string[]>;
  subscribe(listener: (urls: readonly string[]) => void): Promise<() => void>;
}

export function parseReadOnlyDeepLink(value: string): ReadOnlyDeepLinkTarget | null {
  const match = READ_ONLY_WORK_LINK.exec(value);
  const code = match?.[1];
  if (!code) return null;
  return { kind: "work", code: code.toUpperCase() };
}

export async function installReadOnlyDeepLinkNavigation(
  gateway: ReadOnlyDeepLinkGateway,
  navigate: (target: ReadOnlyDeepLinkTarget) => void,
): Promise<() => void> {
  await gateway.readCurrent();

  let readingInitial = true;
  const deliveredWhileReading = new Set<string>();
  const deliver = (urls: readonly string[], remember: boolean) => {
    for (const value of urls) {
      if (remember) deliveredWhileReading.add(value);
      const target = parseReadOnlyDeepLink(value);
      if (target) navigate(target);
    }
  };
  const unlisten = await gateway.subscribe((urls) => deliver(urls, readingInitial));
  try {
    const current = await gateway.readCurrent();
    deliver(current.filter((value) => !deliveredWhileReading.has(value)), false);
    readingInitial = false;
    return unlisten;
  } catch (error) {
    unlisten();
    throw error;
  }
}
