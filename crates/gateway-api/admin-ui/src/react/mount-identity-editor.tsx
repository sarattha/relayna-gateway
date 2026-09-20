import { createRoot } from "react-dom/client";
import { flushSync } from "react-dom";
import {
  IdentityEditor,
  type IdentityDraft,
  type EntraPolicy,
} from "./identity-editor";
import type { KeyCatalog } from "./key-picker";

/** Preserve the form wire contract while giving React sole ownership of fields. */
export function readIdentityDraft(host: HTMLElement): IdentityDraft {
  const value = (name: string) =>
    host.querySelector<HTMLInputElement>(`[name="${CSS.escape(name)}"]`)
      ?.value || "";
  const checked = (name: string) =>
    Boolean(
      host.querySelector<HTMLInputElement>(`[name="${CSS.escape(name)}"]`)
        ?.checked,
    );
  const entra = (prefix: string): EntraPolicy => {
    const name = (field: string) =>
      prefix === "endpoint"
        ? `endpoint_${({ audience: "audience", required_scopes: "scopes", required_roles: "roles", allowed_groups: "groups", allow_apigee: "apigee" } as Record<string, string>)[field]}`
        : `${prefix}.${field}`;
    const list = (field: string) =>
      value(name(field)).split(",").filter(Boolean);
    return {
      audience: value(name("audience")),
      required_scopes: list("required_scopes"),
      required_roles: list("required_roles"),
      allowed_groups: list("allowed_groups"),
      allow_apigee: checked(name("allow_apigee")),
    };
  };
  return {
    mode: value("endpoint_entra_mode"),
    entra: entra("endpoint"),
    revision: Number(value("profiles_revision")),
    profiles: [
      ...host.querySelectorAll<HTMLInputElement>('[name="profile_slot"]'),
    ].map(({ value: slot }) => ({
      slot,
      id: value(`${slot}.id`),
      name: value(`${slot}.name`),
      type: value(`${slot}.type`),
      enabled: checked(`${slot}.enabled`),
      entra: entra(slot),
      keys: value(`${slot}.keys`).split(",").filter(Boolean),
    })),
  };
}
export function mountIdentityEditor(
  host: HTMLElement,
  loadCatalog: () => Promise<KeyCatalog>,
) {
  const initial = readIdentityDraft(host);
  host.dataset.reactUi = "";
  host.style.display = "block";
  const root = createRoot(host);
  flushSync(() =>
    root.render(
      <IdentityEditor
        initial={initial}
        loadCatalog={loadCatalog}
        onDirty={() => {
          const form = host.closest("form");
          if (form) form.dataset.dirty = "true";
        }}
      />,
    ),
  );
  const observer = new MutationObserver(() => {
    if (!host.isConnected) {
      observer.disconnect();
      root.unmount();
    }
  });
  observer.observe(document.body, { childList: true, subtree: true });
  return () => {
    observer.disconnect();
    root.unmount();
  };
}
