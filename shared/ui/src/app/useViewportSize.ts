import { useEffect, useState } from "react";

export type ViewportSize = {
  width: number;
  height: number;
};

export function useViewportSize(): ViewportSize {
  const [size, setSize] = useState(readViewportSize);

  useEffect(() => {
    const update = () => setSize(readViewportSize());
    window.addEventListener("resize", update);
    return () => window.removeEventListener("resize", update);
  }, []);

  return size;
}

function readViewportSize(): ViewportSize {
  return { width: window.innerWidth, height: window.innerHeight };
}
