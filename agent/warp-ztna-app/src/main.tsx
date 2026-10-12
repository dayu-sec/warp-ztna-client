import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { setupLogging } from "./lib/logging";
import "./styles/index.css";

document.documentElement.setAttribute("data-native", "1");
void setupLogging();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
