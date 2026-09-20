import { useState } from "react";
import { createRoot } from "react-dom/client";
import { Button, Dialog, Field, Input, NativeSelect } from "../components/ui";

type Profile = { id: string; name: string; enabled: boolean; type: string };
export type BindingEntry = {
  route: string;
  url: string;
  method: string;
  access: {
    authentication_profiles?: {
      revision: number;
      profiles: Profile[];
      bindings: { key_id: string; profile_id: string }[];
    };
    [key: string]: unknown;
  };
};
type Props = {
  keyId: string;
  keyName: string;
  entries: BindingEntry[];
  save: (entry: BindingEntry, access: BindingEntry["access"]) => Promise<BindingEntry["access"]>;
  onClose: () => void;
  trigger: HTMLElement;
};
const assignment = (entry: BindingEntry, keyId: string) =>
  entry.access?.authentication_profiles?.bindings.find(binding => binding.key_id === keyId)?.profile_id || "";

export function KeyRouteBindings({ keyId, keyName, entries: initial, save, onClose, trigger }: Props) {
  const [entries, setEntries] = useState(initial);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [query, setQuery] = useState("");
  const [pending, setPending] = useState<string | null>(null);
  const [feedback, setFeedback] = useState<Record<string, { error: boolean; text: string }>>({});
  const [discard, setDiscard] = useState(false);
  const dirty = entries.some(entry => (drafts[entry.route] ?? assignment(entry, keyId)) !== assignment(entry, keyId));
  const profiled = entries.filter(entry => entry.access?.authentication_profiles);
  const inherited = entries.filter(entry => !entry.access?.authentication_profiles);
  const needle = query.trim().toLowerCase();
  const matches = (entry: BindingEntry) => entry.route.toLowerCase().includes(needle);
  const close = () => {
    if (pending) return;
    if (dirty) setDiscard(true);
    else onClose();
  };
  async function saveRoute(entry: BindingEntry, profileId: string) {
    setPending(entry.route);
    setFeedback(current => ({ ...current, [entry.route]: { error: false, text: "Saving…" } }));
    const access = structuredClone(entry.access);
    const set = access.authentication_profiles!;
    set.bindings = set.bindings.filter(binding => binding.key_id !== keyId);
    if (profileId) set.bindings.push({ key_id: keyId, profile_id: profileId });
    try {
      const updated = await save(entry, access);
      setEntries(current => current.map(row => row.route === entry.route ? { ...row, access: updated } : row));
      setFeedback(current => ({ ...current, [entry.route]: { error: false, text: "Saved" } }));
    } catch (error) {
      const message = String(error);
      const text = message.includes("authentication_profile_conflict")
        ? "This route changed since you opened it. Close and reopen this dialog, then choose the profile again."
        : `Could not save this route. ${message.replace(/^Error: /, "")} Your selection is kept. Check the reported problem, then retry Save.`;
      setFeedback(current => ({ ...current, [entry.route]: { error: true, text } }));
    } finally {
      setPending(null);
    }
  }
  return <Dialog title="Route assignments" description={keyName} onClose={close} restoreFocus={trigger}
    footer={discard ? <div className="grid gap-3" role="alert">
      <p className="text-xs">Discard unsaved selections? Assignments already saved will stay in effect.</p>
      <div className="flex gap-2"><Button onClick={() => setDiscard(false)}>Keep editing</Button><Button variant="destructive" onClick={onClose}>Discard changes</Button></div>
    </div> : <div className="flex w-full items-center justify-between gap-3">
      <p className="text-xs text-muted-foreground">{pending ? "Saving assignment. Please wait." : dirty ? "Unsaved selections" : "Each Save applies immediately to that route."}</p>
      <Button disabled={Boolean(pending)} onClick={close}>Done</Button>
    </div>}>
    <div className="grid gap-4">
      <p className="text-xs text-muted-foreground">Choose one profile per route for this key. Unassigned or disabled profiles deny access. Key permissions and other policies still apply.</p>
      <Field label="Find a route"><Input type="search" value={query} onChange={event => setQuery(event.target.value)} placeholder="Search route path" /></Field>
      <section aria-label="Routes using profiles">
        <h4 className="text-xs font-medium text-muted-foreground">Routes using profiles ({profiled.length})</h4>
        {profiled.filter(matches).map(entry => {
          const selected = drafts[entry.route] ?? assignment(entry, keyId);
          const profile = entry.access.authentication_profiles!.profiles.find(item => item.id === selected);
          const changed = selected !== assignment(entry, keyId);
          const result = feedback[entry.route];
          return <div key={entry.route} className="binding-route" role="group" aria-label={entry.route}>
            <div className="flex flex-wrap items-center justify-between gap-2">
              <span className="break-all font-mono text-xs">{entry.route}</span>
              <span className="text-xs text-muted-foreground">{changed ? "Unsaved" : !profile ? "Unassigned" : profile.enabled ? "Assigned" : "Profile disabled"}</span>
            </div>
            <div className="binding-controls">
              <Field label="Authentication profile"><NativeSelect className="w-full min-w-0" disabled={Boolean(pending)} value={selected} onChange={event => {
                setDrafts(current => ({ ...current, [entry.route]: event.target.value }));
                setFeedback(current => ({ ...current, [entry.route]: { error: false, text: "" } }));
                setDiscard(false);
              }}>
                <option value="">Unassigned — deny access</option>
                {entry.access.authentication_profiles!.profiles.map(item => <option key={item.id} value={item.id}>{item.name}{item.enabled ? "" : " (disabled)"}</option>)}
              </NativeSelect></Field>
              <Button disabled={Boolean(pending) || !changed} onClick={() => void saveRoute(entry, selected)}>{pending === entry.route ? "Saving…" : "Save"}</Button>
            </div>
            <p className="text-xs text-muted-foreground">{changed && "After saving: "}{!profile ? "This key cannot call this route." : !profile.enabled ? "This profile is disabled. Assigned keys cannot call this route." : profile.type === "relayna_key_only" ? "Requires this Relayna key only." : "Requires this Relayna key and a verified Entra identity."}</p>
            {result?.text && <p role={result.error ? "alert" : "status"} className={`text-xs ${result.error ? "text-destructive" : "text-muted-foreground"}`}>{result.text}</p>}
          </div>;
        })}
        {!profiled.filter(matches).length && <p className="py-4 text-xs text-muted-foreground">{profiled.length ? "No profile routes match your search." : "No routes use profiles yet. Configure authentication profiles in Routes before assigning this key."}</p>}
      </section>
      {inherited.length > 0 && <details className="border-t border-border pt-3">
        <summary className="cursor-pointer text-xs text-muted-foreground">Routes using existing settings ({inherited.length})</summary>
        <p className="my-3 text-xs text-muted-foreground">No profile assignment is needed here. Each route’s existing authentication and access rules apply.</p>
        <ul className="grid gap-2">{inherited.filter(matches).map(entry => <li key={entry.route} className="break-all font-mono text-xs">{entry.route}</li>)}</ul>
        {!inherited.filter(matches).length && <p className="text-xs text-muted-foreground">No routes match your search.</p>}
      </details>}
      <p className="text-xs text-muted-foreground">Assignments include each route’s aliases.</p>
    </div>
  </Dialog>;
}

export function mountKeyRouteBindings(props: Omit<Props, "onClose">) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const close = () => { root.unmount(); host.remove(); };
  root.render(<KeyRouteBindings {...props} onClose={close} />);
  return close;
}
