const outlinePath = "M2962.4 143.2A132.7 132.7 0 0 1 3211.7 143.2L3395.8 647.7A40 40 0 0 1 3372 699L3319.1 718.3A40 40 0 0 1 3267.8 694.4L3105.8 250.7A20 20 0 0 0 3068.3 250.7L2906.3 694.4A40 40 0 0 1 2855 718.3L2802.1 699A40 40 0 0 1 2778.3 647.7Z";
const bookmarkPath = "M3010.3 454.1L3010.3 661.5C3010.3 675.3 3028.2 661.3 3031.2 658.9C3042.9 649.6 3074.3 616.3 3089.2 616.2C3098.1 616.2 3137 651.1 3145.1 657.3C3145.7 657.8 3146.4 658.3 3147.1 658.8C3152.2 662.7 3163.8 670.3 3163.8 665.9L3163.8 449.3C3163.8 447.7 3162.5 440.5 3160.3 440.4L3035 440.4C3026.5 440.2 3010.3 443.7 3010.3 454.1Z";

export function ArchiveGlyph({
  outlineClassName,
  bookmarkClassName,
}: {
  outlineClassName?: string;
  bookmarkClassName?: string;
}) {
  return (
    <svg viewBox="2741.2 48.1 691.71 691.71" aria-hidden="true">
      <path className={outlineClassName} d={outlinePath} />
      <path className={bookmarkClassName} paintOrder="stroke" d={bookmarkPath} />
    </svg>
  );
}
