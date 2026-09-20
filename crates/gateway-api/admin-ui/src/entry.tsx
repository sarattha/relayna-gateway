import { createRoot } from "react-dom/client";
import { flushSync } from "react-dom";
import { ConsoleShell } from "./react/shell";
import "./theme.css";

// Mount synchronously before the operational controllers bind to their host nodes.
// They own #content and dialog subtrees; React owns shell structure and native
// component islands. No renderer reconciles another renderer's descendants.
const root = createRoot(document.getElementById("root")!);
flushSync(() => root.render(<ConsoleShell />));
void import("./main");
