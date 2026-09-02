// Build variant switch.
//
// The Mac App Store build must not carry the ChatGPT brand (App Store Review
// Guideline 5), so it shows a neutral "AI chat" name and drops the OpenAI
// embedding preset — everything else is identical. That used to live on a
// separate `appstore` branch, which meant merging every change twice; it is a
// build-time flag instead:
//
//   npm run build          → public build (ChatGPT shown)
//   npm run build:appstore → App Store build (relabeled)
//
// Set VITE_APP_STORE=1 in the environment to select the App Store variant for
// any command, including `npm run tauri build`.
export const APP_STORE_BUILD = import.meta.env.VITE_APP_STORE === "1";
