import { render } from "solid-js/web";
import App from "./App";
// Apply the persisted theme to <html data-theme> before first paint (no flash).
import "./lib/themes";

// Self-hosted fonts — no network round-trip on startup. Each import pulls
// the WOFF2 files + @font-face declarations bundled by Vite.
import "@fontsource/geist-sans/400.css";
import "@fontsource/geist-sans/500.css";
import "@fontsource/geist-sans/600.css";
import "@fontsource/geist-mono/400.css";
import "@fontsource/geist-mono/500.css";
import "@fontsource/space-mono/400.css";
import "@fontsource/space-mono/700.css";
import "@fontsource/instrument-serif/400.css";
import "@fontsource/instrument-serif/400-italic.css";

const root = document.getElementById("root");
if (!root) throw new Error("missing root element");
render(() => <App />, root);
