import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { GlobalAudioProvider } from "./features/audio/GlobalAudioPlayer";
import { AppSettingsProvider } from "./features/settings/AppSettingsContext";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <AppSettingsProvider>
      <GlobalAudioProvider>
        <App />
      </GlobalAudioProvider>
    </AppSettingsProvider>
  </React.StrictMode>,
);
