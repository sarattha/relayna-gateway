import { useState, type FormEvent } from "react";
import { createRoot } from "react-dom/client";
import { Button, Dialog, Field, Input, NativeSelect } from "../components/ui";
import { FoundryIdentityFields } from "./foundry-editor";

type Props = {
  onSave: (body: Record<string, unknown>) => Promise<void>;
  onClose: () => void;
  restoreFocus?: HTMLElement | null;
};

export function ProviderCreate({ onSave, onClose, restoreFocus }: Props) {
  const [provider, setProvider] = useState("litellm");
  const [headerMode, setHeaderMode] = useState("authorization_bearer");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const foundry = provider === "azure-foundry";
  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    const value = (name: string) => String(data.get(name) || "").trim();
    const body: Record<string, unknown> = { provider, name: value("name"), base_url: value("base_url"), enabled: data.has("enabled") };
    if (foundry) {
      const method = value("foundry_method");
      body.foundry = { method, tenant_id: value("tenant_id") || null, client_id: value("client_id") || null };
      if (method === "client_secret") body.credential = String(data.get("credential") || "");
    } else {
      const secret = String(data.get("credential") || "");
      if (secret) body.credential = secret;
      body.credential_header_mode = headerMode;
      body.credential_header_name = headerMode === "custom_header" ? value("credential_header_name") : null;
      body.credential_header_value_format = headerMode === "custom_header" ? value("credential_header_value_format") : "raw";
    }
    setBusy(true); setError("");
    try { await onSave(body); setBusy(false); onClose(); }
    catch (error) { setError(String(error instanceof Error ? error.message : error)); setBusy(false); }
  };
  return <Dialog title="Create provider" description="Choose a provider and configure its upstream connection." restoreFocus={restoreFocus} onClose={() => { if (!busy) onClose(); }}>
    <form onSubmit={submit} className="grid gap-4">
      <fieldset disabled={busy} className="grid min-w-0 gap-4">
        <Field label="Provider"><NativeSelect value={provider} onChange={event => { setProvider(event.target.value); setError(""); }}>
          <option value="litellm">LiteLLM</option><option value="internal-service">Internal service</option><option value="azure-foundry">Azure Foundry</option>
        </NativeSelect></Field>
        <Field label="Name"><Input name="name" required placeholder={foundry ? "Production Foundry" : "Production gateway"} /></Field>
        <Field label={foundry ? "Project endpoint" : "Endpoint"} help={foundry ? "Copy the project endpoint from Foundry: https://ACCOUNT.services.ai.azure.com/api/projects/PROJECT. Use the project URL, not an agent or deployment URL." : "The upstream base URL reachable from the gateway."}>
          <Input name="base_url" required type="url" placeholder={foundry ? "https://account.services.ai.azure.com/api/projects/project" : "http://litellm:4000"} />
        </Field>
        <label className="flex items-center gap-2 text-xs text-muted-foreground"><input name="enabled" type="checkbox" defaultChecked />Enabled</label>
        {foundry ? <FoundryIdentityFields /> : <div key={provider} className="grid gap-4">
          <Field label="Default credential (optional)" help="Write-only upstream credential. Leave blank if this provider does not require a shared credential."><Input name="credential" type="password" autoComplete="new-password" /></Field>
          <Field label="Credential mode"><NativeSelect value={headerMode} onChange={event => setHeaderMode(event.target.value)}><option value="authorization_bearer">Authorization bearer</option><option value="custom_header">Custom header</option></NativeSelect></Field>
          {headerMode === "custom_header" && <>
            <Field label="Custom header"><Input name="credential_header_name" required placeholder="x-litellm-api-key" /></Field>
            <Field label="Header value" help="Use raw for headers like x-litellm-api-key: credential. Use bearer when the upstream expects Bearer before the credential."><NativeSelect name="credential_header_value_format"><option value="raw">Raw credential</option><option value="bearer">Bearer credential</option></NativeSelect></Field>
          </>}
        </div>}
      </fieldset>
      {error && <p role="alert" className="text-xs text-destructive">{error}</p>}
      <div className="flex gap-2 border-t border-border pt-4"><Button type="submit" variant="default" disabled={busy}>{busy ? "Creating…" : "Create provider"}</Button><Button disabled={busy} onClick={onClose}>Cancel</Button></div>
    </form>
  </Dialog>;
}

export function mountProviderCreate(props: Omit<Props, "onClose">) {
  const host = document.createElement("div"); host.dataset.reactUi = ""; document.body.append(host);
  const root = createRoot(host);
  const close = () => { root.unmount(); host.remove(); };
  root.render(<ProviderCreate {...props} onClose={close} />);
  return close;
}
