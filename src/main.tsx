import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/nunito";
import "./index.css";
import App from "./App";

// Browser-Zoom per Strg+Rad/Pinch außerhalb des Viewers unterbinden (der Viewer zoomt selbst)
addEventListener("wheel", (e) => e.ctrlKey && e.preventDefault(), { passive: false });
document.addEventListener("gesturestart", (e) => e.preventDefault());

if ((window as any).lpDesktop?.platform === "darwin") document.documentElement.classList.add("mac-app");

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
