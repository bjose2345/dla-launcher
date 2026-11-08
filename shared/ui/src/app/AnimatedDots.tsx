/** Three dots that pulse after a word, for something still going on. */
export function AnimatedDots() {
  return (
    <span className="app-ellipsis" aria-hidden="true">
      <i /><i /><i />
    </span>
  );
}
