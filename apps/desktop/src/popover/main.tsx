import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { PopoverApp } from "./PopoverApp";
import "./popover.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <PopoverApp />
  </StrictMode>,
);
