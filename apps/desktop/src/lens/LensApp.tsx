import { useEffect, useState } from "react";
import { onLensUpdate, type LensView } from "../shared/ipc";
import { Lens } from "./Lens";

export function LensApp() {
  const [view, setView] = useState<LensView | null>(null);

  useEffect(() => {
    const unlisten = onLensUpdate(setView);
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  return view?.visible ? <Lens view={view} /> : null;
}
