/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** "1" selects the App Store build variant — see src/lib/brand.ts. */
  readonly VITE_APP_STORE?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
