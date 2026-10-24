import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource/dm-mono/400.css";
import "@fontsource/dm-mono/500.css";
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/shell.css";
import "./styles/render.css";
import "./styles/home.css";
import "./styles/drawing.css";
import "./styles/device.css";
import "./styles/library.css";
import "./styles/studio.css";
import "./styles/guide.css";
import "./styles/install.css";
import { App } from "./App";
import { PrefsProvider } from "./i18n";
import { DataProvider, ReadingsProvider } from "./state/data";
import { LibraryProvider } from "./state/library";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <PrefsProvider>
      <DataProvider>
        <ReadingsProvider>
          <LibraryProvider>
            <App />
          </LibraryProvider>
        </ReadingsProvider>
      </DataProvider>
    </PrefsProvider>
  </StrictMode>,
);
