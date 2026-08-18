import React from "react";
import ReactDOM from "react-dom/client";
import { HashRouter } from "react-router-dom";
import App from "./App";
import { I18nProvider } from "./lib/i18n";
import { RegionProvider } from "./lib/region";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <I18nProvider>
      <RegionProvider>
        <HashRouter>
          <App />
        </HashRouter>
      </RegionProvider>
    </I18nProvider>
  </React.StrictMode>,
);
