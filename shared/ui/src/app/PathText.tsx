import { displayPath } from "../displayPath";

const PATH_MARK = "\u0000path\u0000";

/**
 * A sentence with a path in it, the path set in the code font: Japanese UI
 * fonts draw a backslash as ¥.
 */
export function PathText({ text, path }: { text: (path: string) => string; path: string }) {
  const [before, after = ""] = text(PATH_MARK).split(PATH_MARK);
  return (
    <>
      {before}
      <code className="app-path">{displayPath(path)}</code>
      {after}
    </>
  );
}
