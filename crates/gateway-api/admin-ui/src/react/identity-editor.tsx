import { useEffect, useId, useState, useRef } from "react";
import {
  Button,
  Field,
  Input,
  NativeSelect,
  CheckboxField,
} from "../components/ui";
import { KeyPicker, type KeyCatalog } from "./key-picker";

export type EntraPolicy = {
  audience: string;
  required_scopes: string[];
  required_roles: string[];
  allowed_groups: string[];
  allow_apigee: boolean;
};
export type ProfileDraft = {
  slot: string;
  id: string;
  name: string;
  type: string;
  enabled: boolean;
  entra: EntraPolicy;
  keys: string[];
};
export type IdentityDraft = {
  mode: string;
  entra: EntraPolicy;
  revision: number;
  profiles: ProfileDraft[];
};
const blankEntra = (): EntraPolicy => ({
  audience: "",
  required_scopes: [],
  required_roles: [],
  allowed_groups: [],
  allow_apigee: false,
});
const help = {
  mode: "Choose how this route determines authentication requirements. Gateway setting inherits Settings; Require Entra uses one audience policy; No Entra retains the route’s credential checks. Use authentication profiles selects a profile by its assigned Relayna key. Saved profiles cannot be removed by switching route modes.",
  id: "Optional. Leave blank to generate an ID from the profile name plus a random suffix when saving. Custom IDs use 1–64 letters, numbers, hyphens or underscores and must be unique on this route. Saved IDs are preserved when left blank or when the name changes.",
  name: "Required display name, up to 120 UTF-8 bytes. Unique on this route ignoring case. Renaming preserves key assignments.",
  type: "Choose what callers assigned to this profile must provide. Entra + Relayna key requires both a verified Entra identity and the assigned key. Relayna key only requires the assigned key. Accessa requires Entra + Relayna key.",
  enabled:
    "Allow assigned keys to use this profile. Turning it off blocks those keys without moving them to another profile.",
  audience:
    "Exact expected JWT audience, such as api://employees. Required for Entra; no whitespace, up to 512 bytes. Tenant, issuer and JWKS come from Settings.",
  required_scopes:
    "Optional comma-separated scopes. Every listed scope is required. Blank adds no scope restriction.",
  required_roles:
    "Optional comma-separated roles. Every listed role is required. Blank adds no role restriction.",
  allowed_groups:
    "Optional comma-separated group IDs. At least one must match. Blank adds no group restriction.",
  allow_apigee:
    "Accept HMAC-verified Apigee identity only with the matching audience, future expiry and required claims.",
};
export function IdentityEditor({
  initial,
  loadCatalog,
  onDirty,
}: {
  initial: IdentityDraft;
  loadCatalog: () => Promise<KeyCatalog>;
  onDirty: () => void;
}) {
  const [draft, setDraft] = useState(initial);
  const savedProfiles = initial.mode === "profiles" && initial.revision > 0;
  const [catalog, setCatalog] = useState<KeyCatalog>({ all: [], eligible: [] });
  const [picker, setPicker] = useState<{
    slot: string;
    trigger: HTMLElement;
  } | null>(null);
  const [notice, setNotice] = useState("");
  const [removed, setRemoved] = useState<{ profile: ProfileDraft; index: number } | null>(null);
  const host = useRef<HTMLDivElement>(null);
  const noticeElement = useRef<HTMLParagraphElement>(null);
  useEffect(() => {
    if (notice) {
      noticeElement.current!.focus();
      noticeElement.current!.scrollIntoView({ block: "nearest" });
    }
  }, [notice]);
  const undoButton = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (removed) undoButton.current!.focus();
  }, [removed]);
  const pendingFocus = useRef<string | null>(null);
  useEffect(() => {
    if (pendingFocus.current) {
      host
        .current!.querySelector<HTMLInputElement>(
          `[name="${CSS.escape(pendingFocus.current)}.id"]`,
        )!
        .focus();
      pendingFocus.current = null;
    }
  }, [draft.profiles]);
  useEffect(() => {
    const node = host.current!;
    const show = (event: Event) =>
      setNotice((event as CustomEvent<string>).detail);
    const preset = (event: Event) => {
      const mode = (event as CustomEvent<string>).detail;
      if (savedProfiles) return;
      setDraft((previous) => ({ ...previous, mode, entra: blankEntra() }));
      setNotice("");
    };
    node.addEventListener("profile-error", show);
    node.addEventListener("identity-preset", preset);
    return () => {
      node.removeEventListener("profile-error", show);
      node.removeEventListener("identity-preset", preset);
    };
  }, [savedProfiles]);
  const unique = useId();
  const [sequence, setSequence] = useState(0);
  useEffect(() => {
    let active = true;
    void loadCatalog()
      .then((value) => {
        if (active) setCatalog(value);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [loadCatalog]);
  const update = (next: IdentityDraft) => {
    setDraft(next);
    setNotice("");
    onDirty();
  };
  const patch = (slot: string, changes: Partial<ProfileDraft>) =>
    update({
      ...draft,
      profiles: draft.profiles.map((profile) =>
        profile.slot === slot ? { ...profile, ...changes } : profile,
      ),
    });
  const add = () => {
    const slot = `react-profile-${unique}-${sequence}`;
    pendingFocus.current = slot;
    setSequence(sequence + 1);
    update({
      ...draft,
      profiles: [
        ...draft.profiles,
        {
          slot,
          id: "",
          name: "",
          type: "entra_and_relayna_key",
          enabled: true,
          entra: blankEntra(),
          keys: [],
        },
      ],
    });
  };
  const remove = (profile: ProfileDraft) => {
    setRemoved({ profile, index: draft.profiles.indexOf(profile) });
    update({
      ...draft,
      profiles: draft.profiles.filter((item) => item.slot !== profile.slot),
    });
  };
  const undoRemove = () => {
    const profiles = [...draft.profiles];
    profiles.splice(removed!.index, 0, removed!.profile);
    pendingFocus.current = removed!.profile.slot;
    update({ ...draft, profiles });
    setRemoved(null);
  };
  const current = draft.profiles.find(
    (profile) => profile.slot === picker?.slot,
  );
  return (
    <div ref={host} data-react-ui className="grid gap-5 w-full">
      {savedProfiles && <Input type="hidden" name="profiles_saved" value="true" />}
      <Field label="Route authentication mode" help={help.mode}>
        <NativeSelect
          name="endpoint_entra_mode"
          value={draft.mode}
          aria-describedby={savedProfiles ? `${unique}-mode-restriction` : undefined}
          onChange={(event) => {
            if (!savedProfiles) update({ ...draft, mode: event.target.value });
          }}
        >
          <option value="profiles">Use authentication profiles</option>
          <option value="inherit" disabled={savedProfiles}>Use existing gateway setting</option>
          <option value="required" disabled={savedProfiles}>Require Entra</option>
          <option value="disabled" disabled={savedProfiles}>No Entra</option>
        </NativeSelect>
      </Field>
      {savedProfiles && (
        <p id={`${unique}-mode-restriction`} className="-mt-3 text-xs text-muted-foreground">
          Saved profiles keep this route in profile mode. To use Relayna key only,
          change Profile authentication below.
        </p>
      )}
      <div data-endpoint-entra hidden={draft.mode !== "required"}>
        <EntraFields
          value={draft.entra}
          prefix="endpoint"
          active={draft.mode === "required"}
          onChange={(entra) => update({ ...draft, entra })}
        />
      </div>
      <section data-profile-editor hidden={draft.mode !== "profiles"}>
        <div className="mb-2 flex items-center justify-between gap-4">
          <h4 className="text-sm font-medium">Authentication profiles</h4>
          <Button
            data-add-profile
            onClick={add}
            disabled={draft.mode !== "profiles" || draft.profiles.length >= 32}
          >
            Add profile
          </Button>
        </div>
        <p className="mb-4 text-xs text-muted-foreground">
          A profile can have multiple keys. Each key belongs to one profile per
          route. Unassigned keys are denied.
        </p>
        {draft.profiles.length >= 32 && (
          <p className="mb-4 text-xs text-muted-foreground">32 profiles is the limit. Remove an unused profile before adding another.</p>
        )}
        {removed && (
          <div role="status" className="mb-4 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
            <span>“{removed.profile.name || "New authentication profile"}” removed from this draft. Save identity to apply.</span>
            <Button ref={undoButton} size="sm" onClick={undoRemove} disabled={draft.profiles.length >= 32}>Undo removal</Button>
            {draft.profiles.length >= 32 && <span>32-profile limit reached. Cancel to discard all draft changes.</span>}
          </div>
        )}
        <Input
          type="hidden"
          name="profiles_revision"
          value={draft.revision}
          disabled={draft.mode !== "profiles"}
        />
        {!draft.profiles.length && (
          <p className="rounded-md border border-dashed border-border p-4 text-xs text-muted-foreground">
            No authentication profiles. Use Add profile to configure who can access
            this route. At least one profile is needed before saving.
          </p>
        )}
        <div data-profile-rows className="grid gap-4">
          {draft.profiles.map((profile) => (
            <fieldset
              key={profile.slot}
              data-profile-row
              className="rounded-lg border border-border p-4"
              disabled={draft.mode !== "profiles"}
            >
              <legend className="px-1.5 text-xs font-medium">
                {profile.name || "New authentication profile"}
              </legend>
              <Input type="hidden" name="profile_slot" value={profile.slot} />
              <Input
                type="hidden"
                name={`${profile.slot}.original_id`}
                value={initial.profiles.find((saved) => saved.slot === profile.slot)?.id || ""}
              />
              <div className="grid gap-4 sm:grid-cols-2">
                <Field label="Stable profile ID (optional)" help={help.id}>
                  <Input
                    name={`${profile.slot}.id`}
                    value={profile.id}
                    onChange={(event) =>
                      patch(profile.slot, { id: event.target.value })
                    }
                    maxLength={64}
                    pattern="(?:[A-Za-z0-9_]|-)+"
                    placeholder="Generated from profile name on save"
                  />
                </Field>
                <Field label="Profile name (required)" help={help.name}>
                  <Input
                    name={`${profile.slot}.name`}
                    value={profile.name}
                    onChange={(event) =>
                      patch(profile.slot, { name: event.target.value })
                    }
                    required
                    maxLength={120}
                    placeholder="Internal automation"
                  />
                </Field>
                <Field label="Profile authentication" help={help.type}>
                  <NativeSelect
                    data-profile-type
                    name={`${profile.slot}.type`}
                    value={profile.type}
                    onChange={(event) =>
                      patch(profile.slot, { type: event.target.value })
                    }
                  >
                    <option value="entra_and_relayna_key">
                      Entra + Relayna key
                    </option>
                    <option value="relayna_key_only">Relayna key only</option>
                  </NativeSelect>
                </Field>
                <CheckboxField
                  label="Enabled"
                  help={help.enabled}
                  name={`${profile.slot}.enabled`}
                  checked={profile.enabled}
                  onChange={(event) =>
                    patch(profile.slot, { enabled: event.target.checked })
                  }
                />
              </div>
              <div
                data-profile-entra
                className="mt-4"
                hidden={profile.type !== "entra_and_relayna_key"}
              >
                <EntraFields
                  value={profile.entra}
                  prefix={profile.slot}
                  active={
                    draft.mode === "profiles" &&
                    profile.type === "entra_and_relayna_key"
                  }
                  onChange={(entra) => patch(profile.slot, { entra })}
                />
              </div>
              <p
                data-profile-key-only
                hidden={profile.type !== "relayna_key_only"}
                className="mt-4 text-xs text-muted-foreground"
              >
                Uses the assigned Relayna key. No Entra audience or claims are
                required.
              </p>
              <section className="mt-4" data-key-picker>
                <Input
                  data-profile-keys
                  type="hidden"
                  name={`${profile.slot}.keys`}
                  value={profile.keys.join(",")}
                />
                <div className="flex items-center justify-between gap-3">
                  <span className="text-xs font-medium text-muted-foreground">
                    Assigned keys
                  </span>
                  <Button
                    data-open-keys
                    aria-haspopup="dialog"
                    onClick={(event) =>
                      setPicker({
                        slot: profile.slot,
                        trigger: event.currentTarget,
                      })
                    }
                  >
                    Select keys
                  </Button>
                </div>
                <div data-selected-keys>
                  {profile.keys.length ? (
                    profile.keys.map((id) => {
                      const key = catalog.all.find((item) => item.id === id);
                      return (
                        <div
                          key={id}
                          className="flex items-center justify-between gap-3 border-b border-border py-3"
                        >
                          <div className="min-w-0">
                            <span className="block text-[13px]">
                              {key?.name || "Key details unavailable"}
                            </span>
                            <span className="block text-xs text-muted-foreground">
                              {key?.owner || "Saved assignment"}
                              {key ? ` · ${key.status}` : ""}
                            </span>
                            <code className="block break-all text-[11px] text-muted-foreground">
                              {id}
                            </code>
                          </div>
                          <Button
                            size="sm"
                            aria-label={`Remove ${key?.name || id}`}
                            onClick={() =>
                              patch(profile.slot, {
                                keys: profile.keys.filter((key) => key !== id),
                              })
                            }
                          >
                            Remove
                          </Button>
                        </div>
                      );
                    })
                  ) : (
                    <p className="py-3 text-xs text-muted-foreground">
                      No keys assigned.
                    </p>
                  )}
                </div>
              </section>
              <div className="mt-3 grid justify-items-start gap-2">
                <Button
                  variant="ghost"
                  disabled={Boolean(profile.keys.length) || (savedProfiles && draft.profiles.length === 1)}
                  aria-describedby={`${profile.slot}-remove-help`}
                  onClick={() => remove(profile)}
                >
                  Remove profile
                </Button>
                <p id={`${profile.slot}-remove-help`} className="text-xs text-muted-foreground">
                  {savedProfiles && draft.profiles.length === 1
                    ? "Keep at least one profile on this route. Add a replacement before removing this one. To deny access instead, turn off Enabled and save."
                    : profile.keys.length
                      ? `This profile has ${profile.keys.length} assigned ${profile.keys.length === 1 ? "key" : "keys"}. Remove its Assigned keys first; reassign them to another profile before saving if they still need access.`
                      : "Removal takes effect when you Save identity. Cancel discards your changes."}
                </p>
              </div>
            </fieldset>
          ))}
        </div>
      </section>
      <p
        ref={noticeElement}
        data-profile-notice
        data-react-error
        tabIndex={-1}
        role={notice ? "alert" : "status"}
        className="text-xs text-destructive"
      >
        {notice}
      </p>
      {picker && current && (
        <KeyPicker
          initial={current.keys}
          load={loadCatalog}
          elsewhere={() =>
            new Set(
              draft.profiles
                .filter((profile) => profile.slot !== current.slot)
                .flatMap((profile) => profile.keys),
            )
          }
          onApply={(keys) => patch(current.slot, { keys })}
          onClose={() => setPicker(null)}
          restoreFocus={picker.trigger}
        />
      )}
    </div>
  );
}
function EntraFields({
  value,
  prefix,
  active,
  onChange,
}: {
  value: EntraPolicy;
  prefix: string;
  active: boolean;
  onChange: (value: EntraPolicy) => void;
}) {
  const name = (field: string) =>
    prefix === "endpoint"
      ? `endpoint_${({ audience: "audience", required_scopes: "scopes", required_roles: "roles", allowed_groups: "groups", allow_apigee: "apigee" } as Record<string, string>)[field]}`
      : `${prefix}.${field}`;
  return (
    <fieldset disabled={!active} className="grid gap-4 sm:grid-cols-2">
      <Field
        label={
          prefix === "endpoint"
            ? "Entra audience (required)"
            : "Profile audience (required)"
        }
        help={help.audience}
      >
        <Input
          name={name("audience")}
          value={value.audience}
          onChange={(event) =>
            onChange({ ...value, audience: event.target.value })
          }
          required={active}
          placeholder="api://employees"
        />
      </Field>
      {(["required_scopes", "required_roles", "allowed_groups"] as const).map(
        (field) => (
          <Field
            key={field}
            label={
              {
                required_scopes: "Scopes (optional)",
                required_roles: "Roles (optional)",
                allowed_groups: "Groups (optional)",
              }[field]
            }
            help={help[field]}
          >
            <Input
              name={name(field)}
              value={value[field].join(",")}
              onChange={(event) =>
                onChange({ ...value, [field]: event.target.value.split(",") })
              }
            />
          </Field>
        ),
      )}
      <CheckboxField
        label="Accept signed Apigee identity"
        help={help.allow_apigee}
        name={name("allow_apigee")}
        checked={value.allow_apigee}
        onChange={(event) =>
          onChange({ ...value, allow_apigee: event.target.checked })
        }
      />
    </fieldset>
  );
}
