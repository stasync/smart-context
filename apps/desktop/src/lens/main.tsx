import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { LensApp } from "./LensApp";
import "./lens.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <LensApp />
  </StrictMode>,
);
