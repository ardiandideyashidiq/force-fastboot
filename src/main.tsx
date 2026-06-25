import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import App from "./App.tsx";

const root = document.documentElement;
const media = window.matchMedia("(prefers-color-scheme: dark)");
const stored = window.localStorage.getItem("app-theme");
const initial = stored ?? (media.matches ? "dark" : "light");
root.classList.toggle("dark", initial === "dark");

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
