import { useState } from "react";
import { createRoot } from "react-dom/client";
import { Button, Dialog } from "../components/ui";

type CheckStep = { status: "passed" | "failed" | "inconclusive" | "skipped"; code: string; message: string; http_status: number | null };
export type FoundryCheckResult = {
  provider_id: string; configuration_revision: number; checked_at: string; instance_id: string;
  identity_method: string; identity: CheckStep; project: CheckStep;
};
type Props = {
  name: string; onCheck: () => Promise<FoundryCheckResult>; onClose: () => void;
  restoreFocus?: HTMLElement | null;
};
const labels = { passed: "Verified", failed: "Failed", inconclusive: "Not verified", skipped: "Not checked" };

export function FoundryCheck({ name, onCheck, onClose, restoreFocus }: Props) {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<FoundryCheckResult | null>(null);
  const [error, setError] = useState("");
  const check = async () => {
    setBusy(true); setResult(null); setError("");
    try { setResult(await onCheck()); }
    catch { setError("The gateway could not complete this check. Confirm your session has provider-management permission and the gateway is reachable, then retry."); }
    finally { setBusy(false); }
  };
  return <Dialog title="Verify Foundry connection" description={name} restoreFocus={restoreFocus} onClose={() => { if (!busy) onClose(); }}>
    <div className="grid gap-4">
      <p className="text-sm text-muted-foreground">Checks the saved Azure identity and project read access from this gateway instance. No agent is run. Save configuration changes before checking.</p>
      {busy && <p role="status" className="text-sm">Checking Azure identity and Foundry access…</p>}
      {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
      {result && <div role="status" className="grid gap-4">
        {([["Azure identity", result.identity], ["Foundry project read access", result.project]] as const).map(([title, step]) => <section key={title} className="grid gap-1 border-b border-border pb-3">
          <div className="flex items-center justify-between gap-3 text-sm"><span>{title}</span><span className={step.status === "failed" ? "text-destructive" : "text-muted-foreground"}>{labels[step.status]}{step.http_status !== null && ` · HTTP ${step.http_status}`}</span></div>
          <p className="text-xs text-muted-foreground">{step.message}</p>
        </section>)}
        <p className="text-xs text-muted-foreground">Agent execution, tools and other gateway instances are not verified by this check.</p>
        <div className="text-xs text-muted-foreground break-all">Checked {new Date(result.checked_at).toLocaleString()}<br />Gateway instance: {result.instance_id}</div>
      </div>}
      <div className="flex gap-2 border-t border-border pt-4"><Button variant="default" disabled={busy} onClick={check}>{busy ? "Checking…" : result || error ? "Check again" : "Check connection"}</Button><Button disabled={busy} onClick={onClose}>Close</Button></div>
    </div>
  </Dialog>;
}

export function mountFoundryCheck(props: Omit<Props, "onClose">) {
  const host = document.createElement("div"); host.dataset.reactUi = ""; document.body.append(host);
  const root = createRoot(host);
  const close = () => { root.unmount(); host.remove(); };
  root.render(<FoundryCheck {...props} onClose={close} />);
  return close;
}
