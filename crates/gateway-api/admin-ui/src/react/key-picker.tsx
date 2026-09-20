import { useEffect, useRef, useState } from "react";
import { Button, Dialog, Field, Input } from "../components/ui";

export type KeyOption = {
  id: string;
  name: string;
  owner: string;
  status: string;
  searchable: string;
};
export type KeyCatalog = { all: KeyOption[]; eligible: KeyOption[] };
export type KeyPickerProps = {
  initial: string[];
  load: () => Promise<KeyCatalog>;
  elsewhere: () => Set<string>;
  onApply: (ids: string[]) => void;
  onClose: () => void;
  restoreFocus?: HTMLElement | null;
};
export function matchingKeys(
  catalog: KeyCatalog,
  query: string,
  selected: string[],
  unavailable: Set<string>,
) {
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (!terms.length) return [];
  return catalog.eligible.filter(
    (key) =>
      !selected.includes(key.id) &&
      !unavailable.has(key.id) &&
      terms.every((term) => key.searchable.includes(term)),
  );
}
export function KeyPicker({
  initial,
  load,
  elsewhere,
  onApply,
  onClose,
  restoreFocus,
}: KeyPickerProps) {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(initial);
  const [catalog, setCatalog] = useState<KeyCatalog>({ all: [], eligible: [] });
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [conflict, setConflict] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    let active = true;
    setLoading(true);
    setFailed(false);
    void load()
      .then((value) => {
        if (active) setCatalog(value);
      })
      .catch(() => {
        if (active) setFailed(true);
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [load, attempt]);
  const available = matchingKeys(catalog, query, selected, elsewhere());
  const choose = (id: string) => {
    setSelected((ids) => [...ids, id]);
    setQuery("");
    setConflict(false);
    input.current?.focus();
  };
  const remove = (id: string) => {
    setSelected((ids) => ids.filter((item) => item !== id));
    setConflict(false);
    input.current?.focus();
  };
  const apply = () => {
    const unavailable = elsewhere();
    if (
      selected.some(
        (id) =>
          unavailable.has(id) ||
          (!initial.includes(id) &&
            !catalog.eligible.some((key) => key.id === id)),
      )
    ) {
      setConflict(true);
      return;
    }
    onApply(selected);
    onClose();
  };
  return (
    <Dialog
      initialFocus={input}
      title="Select keys"
      onClose={onClose}
      restoreFocus={restoreFocus}
      footer={
        <>
          <Button
            variant="default"
            onClick={apply}
            disabled={loading || failed}
          >
            Apply selection
          </Button>
          <Button onClick={onClose}>Cancel</Button>
        </>
      }
    >
      <div data-react-ui className="grid gap-4">
        <Field
          label="Search keys"
          help="Search by key name or prefix, UUID, project or service. Keys assigned to another profile on this route are excluded."
        >
          <Input
            ref={input}
            type="search"
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setConflict(false);
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") event.preventDefault();
            }}
            placeholder="Key name, UUID, project or service"
            autoComplete="off"
          />
        </Field>
        <div role="status" className="text-xs text-muted-foreground">
          {loading
            ? "Loading keys…"
            : failed
              ? "Could not load keys. Your selections are kept."
              : !query.trim()
                ? "Type to search keys."
                : available.length
                  ? `${available.length} matching keys${available.length > 30 ? " · showing the first 30; refine your search" : ""}.`
                  : "No available keys match your search."}
        </div>
        {failed && (
          <Button onClick={() => setAttempt((value) => value + 1)}>
            Retry loading keys
          </Button>
        )}
        {!loading && !failed && available.length > 0 && (
          <ul
            aria-label="Search results"
            className="max-h-64 overflow-y-auto divide-y divide-border"
          >
            {available.slice(0, 30).map((key) => (
              <KeyRow
                key={key.id}
                item={key}
                action="Add"
                onClick={() => choose(key.id)}
              />
            ))}
          </ul>
        )}
        <section aria-label="Selected keys">
          <h4 className="mb-2 text-xs font-medium text-muted-foreground">
            Selected keys ({selected.length})
          </h4>
          {selected.length ? (
            <ul className="divide-y divide-border">
              {selected.map((id) => (
                <KeyRow
                  key={id}
                  item={
                    catalog.all.find((key) => key.id === id) || {
                      id,
                      name: "Key details unavailable",
                      owner: "Saved assignment",
                      status: "Unknown",
                      searchable: "",
                    }
                  }
                  action="Remove"
                  onClick={() => remove(id)}
                />
              ))}
            </ul>
          ) : (
            <p className="text-xs text-muted-foreground">No keys selected.</p>
          )}
        </section>
        {conflict && (
          <p role="alert" className="text-xs text-destructive">
            An assignment changed. Remove keys that are assigned elsewhere or no
            longer available.
          </p>
        )}
        <p className="text-xs text-muted-foreground">
          Apply updates this profile’s draft. Save identity to save the
          assignments.
        </p>
      </div>
    </Dialog>
  );
}
function KeyRow({
  item,
  action,
  onClick,
}: {
  item: KeyOption;
  action: string;
  onClick: () => void;
}) {
  return (
    <li className="flex items-center justify-between gap-4 py-3">
      <div className="min-w-0">
        <span className="block break-all text-[13px] font-normal text-foreground">
          {item.name}
        </span>
        <span className="block text-xs text-muted-foreground">
          {item.owner} · {item.status}
        </span>
        <code className="block break-all text-[11px] text-muted-foreground">
          {item.id}
        </code>
      </div>
      <Button
        size="sm"
        onClick={onClick}
        aria-label={`${action} ${item.name} (${item.id})`}
      >
        {action}
      </Button>
    </li>
  );
}
