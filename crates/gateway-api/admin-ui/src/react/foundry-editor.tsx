import { useState, type FormEvent } from "react";
import { createRoot } from "react-dom/client";
import { Button, Dialog, Field, Input, NativeSelect } from "../components/ui";

type Identity = { method: string; tenant_id: string | null; client_id: string | null };
type Binding = { mode: string; provider_id: string; agent_name?: string; agent_version?: string | null };
export type FoundryRecord = { id?: string; name: string; base_url?: string; enabled?: boolean; credential_configured?: boolean; foundry?: Identity | Binding };
type Props = {
  kind: "provider" | "service";
  record?: FoundryRecord;
  providers: FoundryRecord[];
  projects: { id: string; name: string }[];
  onSave: (body: Record<string, unknown>) => Promise<void>;
  onClose: () => void;
  restoreFocus?: HTMLElement | null;
};

export function FoundryIdentityFields({ identity, credentialConfigured = false }: { identity?: Identity; credentialConfigured?: boolean }) {
  const [method, setMethod] = useState(identity?.method || "workload_identity");
  return <>
        <Field label="Azure identity"><NativeSelect name="foundry_method" value={method} onChange={e => setMethod(e.target.value)}>
          <option value="workload_identity">Workload identity (AKS)</option><option value="managed_identity">Managed identity (Azure VM)</option><option value="client_secret">Client secret</option>
        </NativeSelect></Field>
        {method !== "managed_identity" && <Field label="Tenant ID"><Input name="tenant_id" required defaultValue={identity?.tenant_id || ""} /></Field>}
        <Field label={method === "managed_identity" ? "Client ID (optional)" : "Client ID"} help="For a user-assigned identity, enter its client ID. Leave blank for the VM's system-assigned identity."><Input name="client_id" required={method !== "managed_identity"} defaultValue={identity?.client_id || ""} /></Field>
        {method === "client_secret" && <Field label={credentialConfigured ? "Replace client secret (optional)" : "Client secret"}><Input name="credential" type="password" autoComplete="new-password" required={!credentialConfigured} /></Field>}
        {method === "workload_identity" && <p className="text-xs text-muted-foreground">Configure the pod’s federated identity and AZURE_FEDERATED_TOKEN_FILE before invoking this connection.</p>}
  </>;
}

export function FoundryEditor({ kind, record, providers, projects, onSave, onClose, restoreFocus }: Props) {
  const identity = record?.foundry as Identity | undefined;
  const binding = record?.foundry as Binding | undefined;
  const [mode, setMode] = useState(binding?.mode || "registered_agent");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const provider = kind === "provider";
  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    const value = (name: string) => String(data.get(name) || "").trim();
    let body: Record<string, unknown>;
    if (provider) {
      body = {
        name: value("name"), base_url: value("base_url"),
        foundry: { method: value("foundry_method"), tenant_id: value("tenant_id") || null, client_id: value("client_id") || null },
        ...(record ? {} : { provider: "azure-foundry", enabled: true }),
      };
      const secret = String(data.get("credential") || "");
      if (secret) body.credential = secret;
    } else {
      body = {
        foundry: { mode, provider_id: value("provider_id"), ...(mode === "registered_agent" ? { agent_name: value("agent_name"), agent_version: value("agent_version") || null } : {}) },
        ...(record ? {} : {
          name: value("name"), project_id: value("project_id") || null,
          route_pattern: `/services/${value("name")}/*`, allowed_methods: ["POST"],
          timeout_ms: 120000, max_body_bytes: 1048576, access: { skip_entra: value("caller_auth") === "key_only" },
        }),
      };
    }
    setBusy(true); setError("");
    try { await onSave(body); setBusy(false); onClose(); }
    catch (error) { setError(String(error instanceof Error ? error.message : error)); setBusy(false); }
  };
  return <Dialog title={provider ? "Azure Foundry connection" : "Foundry service"} onClose={() => { if (!busy) onClose(); }} restoreFocus={restoreFocus}
    description={provider ? "Azure credentials are used only by the gateway. Caller authentication is configured on each service." : "Register an existing agent or expose the project Responses endpoint."}>
    <form onSubmit={submit} className="grid gap-4">
      {(provider || !record) && <Field label="Name"><Input name="name" required defaultValue={record?.name || ""} pattern={provider ? undefined : "[a-z0-9][a-z0-9-]{0,62}[a-z0-9]|[a-z0-9]"} placeholder={provider ? "Production Foundry" : "research-agent"} /></Field>}
      {provider ? <>
        <Field label="Project endpoint" help="Copy the project endpoint from Foundry. Use https://ACCOUNT.services.ai.azure.com/api/projects/PROJECT. Do not paste an agent or model deployment URL."><Input name="base_url" required type="url" defaultValue={record?.base_url || ""} placeholder="https://account.services.ai.azure.com/api/projects/project" /></Field>
        <FoundryIdentityFields identity={identity} credentialConfigured={record?.credential_configured} />
      </> : <>
        <Field label="Foundry connection"><NativeSelect name="provider_id" required defaultValue={binding?.provider_id || ""}><option value="">Choose a connection…</option>{providers.map(p => <option key={p.id} value={p.id}>{p.name}{p.enabled ? "" : " (disabled)"}</option>)}</NativeSelect></Field>
        {!providers.length && <p role="status" className="text-xs text-muted-foreground">Add an Azure Foundry connection in Providers first.</p>}
        <Field label="Integration mode"><NativeSelect value={mode} onChange={e => setMode(e.target.value)}><option value="registered_agent">Registered agent</option><option value="endpoint_passthrough">Foundry endpoint passthrough</option></NativeSelect></Field>
        {mode === "registered_agent" ? <>
          <Field label="Agent name" help="The existing Foundry agent's API name, not its display label or classic assistant ID. Use 1–128 letters, digits, hyphens or underscores. Callers cannot override this agent."><Input name="agent_name" pattern="[A-Za-z0-9_-]{1,128}" title="Use 1–128 letters, digits, hyphens or underscores." required defaultValue={binding?.agent_name || ""} /></Field>
          <Field label="Agent version (optional)" help="Pin a version for predictable deployments. Blank uses Foundry's default version selection."><Input name="agent_version" pattern="[A-Za-z0-9_-]{1,128}" title="Use 1–128 letters, digits, hyphens or underscores, or leave blank." defaultValue={binding?.agent_version || ""} /></Field>
        </> : <p className="text-xs text-muted-foreground">Clients supply the model or agent reference. Only stateless POST responses is exposed; Azure management and conversation APIs remain closed.</p>}
        {!record && <>
          <Field label="Project (optional)"><NativeSelect name="project_id"><option value="">No project</option>{projects.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</NativeSelect></Field>
          <Field label="Caller authentication"><NativeSelect name="caller_auth"><option value="inherit">Use gateway settings</option><option value="key_only">Relayna key only</option></NativeSelect></Field>
          <p className="text-xs text-muted-foreground">Invoke /services/NAME/responses with a permitted Relayna key. After creating the service, use Edit to configure authentication profiles, limits and pricing.</p>
        </>}
      </>}
      {error && <p role="alert" className="text-xs text-destructive">{error}</p>}
      <div className="flex gap-2 border-t border-border pt-4"><Button type="submit" variant="default" disabled={busy || (!provider && !providers.length)}>{busy ? "Saving…" : "Save"}</Button><Button disabled={busy} onClick={onClose}>Cancel</Button></div>
    </form>
  </Dialog>;
}

export function mountFoundryEditor(props: Omit<Props, "onClose">) {
  const host = document.createElement("div"); host.dataset.reactUi = ""; document.body.append(host);
  const root = createRoot(host);
  const close = () => { root.unmount(); host.remove(); };
  root.render(<FoundryEditor {...props} onClose={close} />);
  return close;
}
