import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { api } from "./api";

interface RegionContextValue {
  /** True when the system region is mainland China (ChatGPT features hidden). */
  isChina: boolean;
}

const RegionContext = createContext<RegionContextValue>({ isChina: false });

export function RegionProvider({ children }: { children: ReactNode }) {
  const [isChina, setIsChina] = useState(false);

  useEffect(() => {
    api.isChinaRegion().then(setIsChina).catch(() => {});
  }, []);

  return <RegionContext.Provider value={{ isChina }}>{children}</RegionContext.Provider>;
}

export function useRegion() {
  return useContext(RegionContext);
}
