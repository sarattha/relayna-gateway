import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";
import {
  KeyPicker,
  matchingKeys,
  type KeyOption,
  type KeyCatalog,
} from "../crates/gateway-api/admin-ui/src/react/key-picker";
import {
  IdentityEditor,
  type IdentityDraft,
} from "../crates/gateway-api/admin-ui/src/react/identity-editor";
import {
  mountIdentityEditor,
  readIdentityDraft,
} from "../crates/gateway-api/admin-ui/src/react/mount-identity-editor";
import { ConsoleShell } from "../crates/gateway-api/admin-ui/src/react/shell";
import {
  Button,
  Dialog,
  Field,
  Help,
  Input,
  NativeSelect,
  cn,
} from "../crates/gateway-api/admin-ui/src/components/ui";

const key = (
  id: string,
  name = `key-${id}`,
  owner = "Research",
): KeyOption => ({
  id,
  name,
  owner,
  status: "Active",
  searchable: `${name} ${id} ${owner} service-ocr`.toLowerCase(),
});
const catalog: KeyCatalog = {
  all: [key("a"), key("b"), key("c")],
  eligible: [key("a"), key("b")],
};
const policy = () => ({
  audience: "api://employees",
  required_scopes: ["read"],
  required_roles: [],
  allowed_groups: [],
  allow_apigee: false,
});
const draft = (): IdentityDraft => ({
  mode: "profiles",
  entra: policy(),
  revision: 5,
  profiles: [
    {
      slot: "employee",
      id: "employees",
      name: "Employees",
      type: "entra_and_relayna_key",
      enabled: true,
      entra: policy(),
      keys: ["a"],
    },
    {
      slot: "automation",
      id: "automation",
      name: "Automation",
      type: "relayna_key_only",
      enabled: false,
      entra: policy(),
      keys: [],
    },
  ],
});
const search = () => screen.getByRole("searchbox", { name: "Search keys" });
async function popup(
  options: Partial<React.ComponentProps<typeof KeyPicker>> = {},
) {
  const props = {
    initial: [],
    load: vi.fn(async () => catalog),
    elsewhere: () => new Set<string>(),
    onApply: vi.fn(),
    onClose: vi.fn(),
    ...options,
  };
  const result = render(<KeyPicker {...props} />);
  await waitFor(() =>
    expect(
      screen
        .getByRole("button", { name: "Apply selection" })
        .hasAttribute("disabled"),
    ).toBe(false),
  );
  return { ...result, props };
}
describe("key selection behavior", () => {
  it("requires a nonblank query; searches all terms without case sensitivity and excludes unavailable keys", () => {
    expect(matchingKeys(catalog, "  ", [], new Set())).toEqual([]);
    expect(
      matchingKeys(catalog, "RESEARCH key-a", [], new Set()).map((x) => x.id),
    ).toEqual(["a"]);
    expect(matchingKeys(catalog, "service-ocr", ["a"], new Set(["b"]))).toEqual(
      [],
    );
    expect(matchingKeys(catalog, "key-c", [], new Set())).toEqual([]);
  });
  it("clears results on manual clear, whitespace and Add while retaining selected keys", async () => {
    const { props } = await popup();
    expect(screen.getByText("Type to search keys.")).toBeTruthy();
    expect(screen.queryByRole("list", { name: "Search results" })).toBeNull();
    fireEvent.change(search(), { target: { value: "Research" } });
    expect(screen.getAllByRole("button", { name: /^Add key/ })).toHaveLength(2);
    fireEvent.change(search(), { target: { value: "" } });
    expect(screen.queryByRole("list", { name: "Search results" })).toBeNull();
    fireEvent.change(search(), { target: { value: "  " } });
    expect(screen.getByText("Type to search keys.")).toBeTruthy();
    fireEvent.change(search(), { target: { value: "key-b" } });
    fireEvent.click(screen.getByRole("button", { name: "Add key-b (b)" }));
    expect((search() as HTMLInputElement).value).toBe("");
    expect(screen.queryByRole("list", { name: "Search results" })).toBeNull();
    expect(screen.getByText("Selected keys (1)")).toBeTruthy();
    expect(props.onApply).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Apply selection" }));
    expect(props.onApply).toHaveBeenCalledWith(["b"]);
    expect(props.onClose).toHaveBeenCalledOnce();
  });
  it("stages removals and Cancel does not update parent", async () => {
    const { props } = await popup({ initial: ["a"] });
    fireEvent.click(screen.getByRole("button", { name: "Remove key-a (a)" }));
    expect(screen.getByText("No keys selected.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(props.onApply).not.toHaveBeenCalled();
    expect(props.onClose).toHaveBeenCalledOnce();
  });
  it("prevents Enter submitting, closes on Escape, and restores originating focus", async () => {
    const trigger = document.createElement("button");
    document.body.append(trigger);
    trigger.focus();
    const { props, unmount } = await popup({ restoreFocus: trigger });
    expect(fireEvent.keyDown(search(), { key: "Enter" })).toBe(false);
    expect(fireEvent.keyDown(search(), { key: "a" })).toBe(true);
    fireEvent.keyDown(search(), { key: "Escape" });
    expect(props.onClose).toHaveBeenCalledOnce();
    unmount();
    await waitFor(() => expect(document.activeElement).toBe(trigger));
    trigger.remove();
  });
  it("shows no-match, applies unknown saved IDs, and treats labels as text", async () => {
    const hostile = key("a", "<img src=x onerror=alert(1)>");
    const { props } = await popup({
      initial: ["unknown", "a"],
      load: async () => ({ all: [hostile], eligible: [hostile] }),
    });
    expect(screen.getByText("Key details unavailable")).toBeTruthy();
    expect(document.querySelector("img")).toBeNull();
    fireEvent.change(search(), { target: { value: "nothing" } });
    expect(
      screen.getByText(/No available keys match your search.*Try a different name/),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Apply selection" }));
    expect(props.onApply).toHaveBeenCalledWith(["unknown", "a"]);
  });
  it("limits long lists and preserves the full selection across queries", async () => {
    const all = Array.from({ length: 35 }, (_, i) => key(String(i)));
    await popup({ load: async () => ({ all, eligible: all }) });
    fireEvent.change(search(), { target: { value: "key" } });
    expect(screen.getByText(/35 matching keys.*first 30/)).toBeTruthy();
    expect(screen.getAllByRole("button", { name: /^Add key/ })).toHaveLength(
      30,
    );
  });
  it("rejects an assignment conflict detected at Apply", async () => {
    const taken = new Set<string>();
    const { props } = await popup({ elsewhere: () => taken });
    fireEvent.change(search(), { target: { value: "key-a" } });
    fireEvent.click(screen.getByRole("button", { name: "Add key-a (a)" }));
    taken.add("a");
    fireEvent.click(screen.getByRole("button", { name: "Apply selection" }));
    expect(screen.getByRole("alert").textContent).toMatch(/key-a \(a\) is assigned to another profile.*Remove it here/);
    expect(props.onApply).not.toHaveBeenCalled();
    fireEvent.change(search(), { target: { value: "another search" } });
    expect(screen.getByRole("alert").textContent).toMatch(/key-a/);
    fireEvent.click(screen.getByRole("button", { name: "Remove key-a (a)" }));
    expect(screen.queryByRole("alert")).toBeNull();
  });
  it("identifies a key that becomes unavailable before Apply", async () => {
    const changing = { all: [key("a")], eligible: [key("a")] };
    const { props } = await popup({ load: async () => changing });
    fireEvent.change(search(), { target: { value: "key-a" } });
    fireEvent.click(screen.getByRole("button", { name: "Add key-a (a)" }));
    changing.eligible = []; changing.all = [];
    fireEvent.click(screen.getByRole("button", { name: "Apply selection" }));
    expect(screen.getByRole("alert").textContent).toMatch(/a is no longer available.*reopen Select keys/);
    expect(props.onApply).not.toHaveBeenCalled();
  });
  it("keeps selections during failure, disables Apply and supports retry", async () => {
    const load = vi
      .fn()
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce(catalog);
    render(
      <KeyPicker
        initial={["a"]}
        load={load}
        elsewhere={() => new Set()}
        onApply={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText("Loading keys…")).toBeTruthy();
    await screen.findByRole("button", { name: "Retry loading keys" });
    expect(
      screen
        .getByRole("button", { name: "Apply selection" })
        .hasAttribute("disabled"),
    ).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Retry loading keys" }));
    await screen.findByText("key-a");
    expect(screen.getByText("Selected keys (1)")).toBeTruthy();
  });
  it("ignores late loads after close", async () => {
    let resolve!: (value: KeyCatalog) => void;
    const onApply = vi.fn();
    const { unmount } = render(
      <KeyPicker
        initial={[]}
        load={() =>
          new Promise((r) => {
            resolve = r;
          })
        }
        elsewhere={() => new Set()}
        onApply={onApply}
        onClose={vi.fn()}
      />,
    );
    unmount();
    await act(async () => resolve(catalog));
    expect(onApply).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
describe("identity editor", () => {
  async function editor(
    initial = draft(),
    loadCatalog = vi.fn(async () => catalog),
  ) {
    const onDirty = vi.fn();
    const result = render(
      <form>
        <IdentityEditor
          initial={initial}
          loadCatalog={loadCatalog}
          onDirty={onDirty}
        />
      </form>,
    );
    await waitFor(() => expect(loadCatalog).toHaveBeenCalled());
    return {
      ...result,
      onDirty,
      form: result.container.querySelector("form")!,
    };
  }
  it("serializes enabled profile fields and omits inactive modes while preserving their drafts", async () => {
    const initial = draft();
    initial.revision = 0;
    const { form } = await editor(initial);
    let data = new FormData(form);
    expect(data.get("profiles_revision")).toBe("0");
    expect(data.getAll("profile_slot")).toEqual(["employee", "automation"]);
    expect(data.has("automation.audience")).toBe(false);
    expect(data.has("endpoint_audience")).toBe(false);
    fireEvent.change(
      screen.getByRole("combobox", { name: "Route authentication mode" }),
      { target: { value: "required" } },
    );
    data = new FormData(form);
    expect(data.get("endpoint_audience")).toBe("api://employees");
    expect(data.getAll("profile_slot")).toEqual([]);
    for (const mode of ["disabled", "inherit"]) {
      fireEvent.change(
        screen.getByRole("combobox", { name: "Route authentication mode" }),
        { target: { value: mode } },
      );
      data = new FormData(form);
      expect(data.has("endpoint_audience")).toBe(false);
      expect(data.has("employee.audience")).toBe(false);
    }
    fireEvent.change(
      screen.getByRole("combobox", { name: "Route authentication mode" }),
      { target: { value: "profiles" } },
    );
    expect(new FormData(form).get("employee.audience")).toBe("api://employees");
  });
  it("explains saved-profile restrictions and preserves assignments through mode and preset attempts", async () => {
    const { form, onDirty } = await editor();
    const mode = screen.getByRole("combobox", { name: "Route authentication mode" });
    expect(document.getElementById(mode.getAttribute("aria-describedby")!)?.textContent?.replace(/\s+/g, " ")).toContain("To use Relayna key only, change Profile authentication below.");
    const before = [...new FormData(form).entries()];
    expect(new FormData(form).get("profiles_saved")).toBe("true");
    for (const value of ["inherit", "required", "disabled"]) {
      expect((mode.querySelector(`[value="${value}"]`) as HTMLOptionElement).disabled).toBe(true);
      fireEvent.change(mode, { target: { value } });
      fireEvent(mode, new CustomEvent("identity-preset", { bubbles: true, detail: value }));
      expect([...new FormData(form).entries()]).toEqual(before);
    }
    expect(onDirty).not.toHaveBeenCalled();
    const stale = "Authentication configuration changed. Reload before saving.";
    fireEvent(mode, new CustomEvent("profile-error", { bubbles: true, detail: stale }));
    expect(screen.getByText(stale)).toBeTruthy();
    expect(within(mode).getByRole("option", { name: "Use authentication profiles" })).toBeTruthy();
    const profileAuth = screen.getAllByRole("combobox", { name: "Profile authentication" })[0];
    expect((profileAuth as HTMLSelectElement).disabled).toBe(false);
    fireEvent.change(profileAuth, { target: { value: "relayna_key_only" } });
    expect(new FormData(form).get("employee.type")).toBe("relayna_key_only");
    expect(new FormData(form).has("employee.audience")).toBe(false);
    expect(new FormData(form).get("endpoint_entra_mode")).toBe("profiles");
  });
  it("guides operators through an empty profile draft and clears the prompt after Add", async () => {
    const initial = draft();
    initial.revision = 0;
    initial.profiles = [];
    await editor(initial);
    expect(screen.getByText(/No authentication profiles\. Use Add profile/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Add profile" }));
    expect(screen.queryByText(/No authentication profiles\. Use Add profile/)).toBeNull();
    expect(screen.getByRole("group", { name: "New authentication profile" })).toBeTruthy();
  });
  it("updates all claim and identity fields and changes key-only mode dynamically", async () => {
    const { form, onDirty } = await editor();
    const group = screen.getByRole("group", { name: "Employees" });
    const fields = [
      ["Stable profile ID (optional)", "staff"],
      ["Profile name (required)", "Staff"],
      ["Profile audience (required)", "api://staff"],
      ["Scopes (optional)", "read,write"],
      ["Roles (optional)", "Invoke"],
      ["Groups (optional)", "team"],
    ];
    for (const [name, value] of fields)
      fireEvent.change(within(group).getByRole("textbox", { name }), {
        target: { value },
      });
    fireEvent.click(within(group).getByRole("checkbox", { name: /Enabled/ }));
    fireEvent.click(
      within(group).getByRole("checkbox", { name: /Accept signed/ }),
    );
    let data = new FormData(form);
    expect(data.get("employee.required_scopes")).toBe("read,write");
    expect(data.has("employee.enabled")).toBe(false);
    expect(data.has("employee.allow_apigee")).toBe(true);
    fireEvent.change(
      within(group).getByRole("combobox", { name: "Profile authentication" }),
      { target: { value: "relayna_key_only" } },
    );
    expect(new FormData(form).has("employee.audience")).toBe(false);
    fireEvent.change(
      within(group).getByRole("combobox", { name: "Profile authentication" }),
      { target: { value: "entra_and_relayna_key" } },
    );
    expect(new FormData(form).get("employee.audience")).toBe("api://staff");
    expect(onDirty).toHaveBeenCalled();
  });
  it("makes profile IDs optional while retaining saved IDs through name edits", async () => {
    const { form } = await editor();
    const group = screen.getByRole("group", { name: "Employees" });
    const id = within(group).getByRole("textbox", { name: "Stable profile ID (optional)" }) as HTMLInputElement;
    expect(id.required).toBe(false);
    fireEvent.change(id, {target:{value:""}});
    fireEvent.change(within(group).getByRole("textbox", {name:"Profile name (required)"}), {target:{value:"Renamed employees"}});
    const data = new FormData(form);
    expect(data.get("employee.original_id")).toBe("employees");
    expect(data.get("employee.id")).toBe("");
    fireEvent.click(screen.getByRole("button", {name:"Add profile"}));
    const fresh = screen.getByRole("group", {name:"New authentication profile"});
    const freshId = within(fresh).getByRole("textbox", {name:"Stable profile ID (optional)"}) as HTMLInputElement;
    expect(freshId.required).toBe(false);
    expect(freshId.placeholder).toMatch(/Generated from profile name/);
    const freshSlot = new FormData(form).getAll("profile_slot").at(-1);
    expect(new FormData(form).get(`${freshSlot}.original_id`)).toBe("");
  });
  it("adds and removes unbound profiles but protects bound profiles", async () => {
    const { form } = await editor();
    const employees = within(screen.getByRole("group", { name: "Employees" }));
    expect(employees.getByRole("button", { name: "Remove profile" }).hasAttribute("disabled")).toBe(true);
    expect(employees.getByText(/This profile has 1 assigned key.*Remove its Assigned keys first/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Add profile" }));
    expect(new FormData(form).getAll("profile_slot")).toHaveLength(3);
    expect(document.activeElement).toBe(
      within(
        screen.getByRole("group", { name: "New authentication profile" }),
      ).getByRole("textbox", { name: "Stable profile ID (optional)" }),
    );
    fireEvent.click(
      within(
        screen.getByRole("group", { name: "New authentication profile" }),
      ).getByRole("button", { name: "Remove profile" }),
    );
    expect(new FormData(form).getAll("profile_slot")).toHaveLength(2);
    fireEvent.click(
      within(screen.getByRole("group", { name: "Employees" })).getByRole(
        "button",
        { name: /Remove key-a/ },
      ),
    );
    expect(new FormData(form).get("employee.keys")).toBe("");
  });
  it("prevents removing the last saved profile and allows Undo of draft removal", async () => {
    const initial = draft(); initial.profiles[0].keys = [];
    const { form } = await editor(initial);
    const removeEmployee = within(screen.getByRole("group", { name: "Employees" })).getByRole("button", { name: "Remove profile" });
    fireEvent.click(removeEmployee);
    expect(new FormData(form).getAll("profile_slot")).toEqual(["automation"]);
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Undo removal" }));
    expect(screen.getByRole("button", { name: "Remove profile" }).hasAttribute("disabled")).toBe(true);
    expect(screen.getByText(/Keep at least one profile.*Add a replacement/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Undo removal" }));
    expect(new FormData(form).getAll("profile_slot")).toEqual(["employee", "automation"]);
    expect(new FormData(form).get("employee.audience")).toBe("api://employees");
    expect(document.activeElement).toBe(within(screen.getByRole("group", { name: "Employees" })).getByRole("textbox", { name: "Stable profile ID (optional)" }));
    expect(screen.queryByRole("button", { name: "Undo removal" })).toBeNull();
  });
  it("keeps removal reversible for new drafts and explains the profile limit", async () => {
    const initial = draft(); initial.revision = 0; initial.profiles = [];
    const { form, unmount } = await editor(initial);
    fireEvent.click(screen.getByRole("button", { name: "Add profile" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove profile" }));
    expect(new FormData(form).getAll("profile_slot")).toHaveLength(0);
    expect(screen.getByText(/“New authentication profile” removed/)).toBeTruthy();
    unmount();
    initial.profiles = Array.from({ length: 31 }, (_, i) => ({ ...draft().profiles[1], slot: `slot-${i}`, id: `profile-${i}`, name: `Profile ${i}` }));
    initial.profiles[0].keys = ["a", "b"];
    await editor(initial);
    expect(screen.getByText(/This profile has 2 assigned keys/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Add profile" }));
    fireEvent.click(within(screen.getByRole("group", { name: "New authentication profile" })).getByRole("button", { name: "Remove profile" }));
    fireEvent.click(screen.getByRole("button", { name: "Add profile" }));
    expect(screen.getByRole("button", { name: "Add profile" }).hasAttribute("disabled")).toBe(true);
    expect(screen.getByRole("button", { name: "Undo removal" }).hasAttribute("disabled")).toBe(true);
    expect(screen.getByText(/32-profile limit reached.*Cancel to discard/)).toBeTruthy();
  });
  it("integrates popup drafts, excludes keys assigned elsewhere and updates hidden bindings on Apply", async () => {
    const { form } = await editor();
    const group = screen.getByRole("group", { name: "Automation" });
    fireEvent.click(within(group).getByRole("button", { name: "Select keys" }));
    await screen.findByText("Type to search keys.");
    fireEvent.change(search(), { target: { value: "key" } });
    expect(screen.queryByRole("button", { name: "Add key-a (a)" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Add key-b (b)" }));
    expect(new FormData(form).get("automation.keys")).toBe("");
    fireEvent.click(screen.getByRole("button", { name: "Apply selection" }));
    expect(new FormData(form).get("automation.keys")).toBe("b");
  });
  it("retains unknown assignments and handles a catalog failure", async () => {
    const initial = draft();
    initial.profiles[0].keys = ["missing"];
    await editor(
      initial,
      vi.fn(async () => {
        throw new Error("offline");
      }),
    );
    expect(screen.getByText("Key details unavailable")).toBeTruthy();
    expect(screen.getByText("Saved assignment")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Remove missing" }));
    expect(screen.queryByText("Saved assignment")).toBeNull();
  });
  it("applies service preset resets to React state without restoring old claims", async () => {
    const initial = draft();
    initial.mode = "required";
    const { form } = await editor(initial);
    fireEvent.change(
      screen.getByRole("textbox", { name: "Entra audience (required)" }),
      { target: { value: "api://old" } },
    );
    const mode = screen.getByRole("combobox", { name: "Route authentication mode" });
    fireEvent(
      mode,
      new CustomEvent("identity-preset", { bubbles: true, detail: "disabled" }),
    );
    expect(new FormData(form).get("endpoint_entra_mode")).toBe("disabled");
    fireEvent.change(mode, { target: { value: "required" } });
    expect(new FormData(form).get("endpoint_audience")).toBe("");
  });
  it("updates the single Entra policy", async () => {
    const initial = draft();
    initial.mode = "required";
    const { form } = await editor(initial);
    fireEvent.change(
      screen.getByRole("textbox", { name: "Entra audience (required)" }),
      { target: { value: "api://route" } },
    );
    expect(new FormData(form).get("endpoint_audience")).toBe("api://route");
  });
});
describe("shared console and accessibility", () => {
  it("renders all Admin and Owner destinations, login and fixed asset links", () => {
    const { container } = render(<ConsoleShell />);
    expect(container.querySelectorAll("[data-view]")).toHaveLength(18);
    expect(screen.getByLabelText("Operator token")).toBeTruthy();
    expect(screen.getByText("Gateway console · 4.0")).toBeTruthy();
    expect(
      container.querySelector("#entra-sign-in")?.getAttribute("href"),
    ).toBe("/admin-ui/auth/login");
    expect(container.querySelector("#content")).toBeTruthy();
  });
  it("composes variant buttons, slots, labeled fields and tap-accessible help", async () => {
    const user = userEvent.setup();
    render(
      <>
        <Button asChild variant="destructive">
          <a href="#test">Delete</a>
        </Button>
        <Field label="Example">
          <Input />
        </Field>
        <NativeSelect aria-label="Choice">
          <option>A</option>
        </NativeSelect>
        <Help label="Example">Explanation</Help>
      </>,
    );
    expect(screen.getByRole("link", { name: "Delete" })).toBeTruthy();
    expect(screen.getByRole("textbox", { name: "Example" })).toBeTruthy();
    expect(cn("p-2", "p-4")).toBe("p-4");
    await user.click(screen.getByRole("button", { name: "About Example" }));
    await waitFor(() =>
      expect(screen.getByRole("tooltip").textContent).toBe("Explanation"),
    );
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("tooltip")).toBeNull();
  });
  it("closes a described dialog with its close control", () => {
    const close = vi.fn();
    render(
      <Dialog title="Confirm" description="Review this change" onClose={close}>
        Content
      </Dialog>,
    );
    expect(screen.getByRole("dialog").getAttribute("aria-describedby")).toBe(
      screen.getByText("Review this change").id,
    );
    fireEvent.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(close).toHaveBeenCalledOnce();
  });
  it("reads legacy form contracts before transferring field ownership and cleans up on removal", async () => {
    const form = document.createElement("form");
    const host = document.createElement("div");
    form.append(host);
    document.body.append(form);
    host.innerHTML =
      '<select name="endpoint_entra_mode"><option selected value="required">Required</option></select><input name="endpoint_audience" value="api://route"><input name="profiles_revision" value="3"><input name="profile_slot" value="x"><input name="x.id" value="employee"><input name="x.name" value="Employee"><input name="x.type" value="relayna_key_only"><input name="x.enabled" type="checkbox" checked><input name="x.keys" value="a,b">';
    expect(readIdentityDraft(host).profiles[0].keys).toEqual(["a", "b"]);
    let cleanup!: () => void;
    await act(async () => {
      cleanup = mountIdentityEditor(host, async () => catalog);
    });
    fireEvent.change(
      screen.getByRole("textbox", { name: "Entra audience (required)" }),
      { target: { value: "api://new" } },
    );
    expect(form.dataset.dirty).toBe("true");
    expect(new FormData(form).get("endpoint_audience")).toBe("api://new");
    await act(async () => {
      form.remove();
    });
    cleanup();
  });
});

describe("lifecycle and server error regressions", () => {
  it("shows server profile errors through React state and clears them on edit", async () => {
    const result = render(
      <IdentityEditor
        initial={draft()}
        loadCatalog={async () => catalog}
        onDirty={vi.fn()}
      />,
    );
    await act(async () => {});
    const notice = result.container.querySelector("[data-profile-notice]")!;
    const scrolledText: string[] = [];
    const scroll = vi.spyOn(notice, "scrollIntoView").mockImplementation(() => {
      scrolledText.push(notice.textContent || "");
    });
    act(() =>
      notice.dispatchEvent(
        new CustomEvent("profile-error", {
          bubbles: true,
          detail: "Revision conflict. Reload and try again.",
        }),
      ),
    );
    expect(screen.getByRole("alert").textContent).toMatch(/Revision conflict/);
    expect(document.activeElement).toBe(notice);
    expect(scrolledText).toEqual(["Revision conflict. Reload and try again."]);
    scroll.mockRestore();
    fireEvent.change(
      screen.getAllByRole("textbox", { name: "Profile name (required)" })[0],
      { target: { value: "Changed" } },
    );
    expect(screen.queryByRole("alert")).toBeNull();
  });
  it("ignores identity metadata resolving after unmount", async () => {
    let resolve!: (catalog: KeyCatalog) => void;
    const result = render(
      <IdentityEditor
        initial={draft()}
        loadCatalog={() =>
          new Promise((r) => {
            resolve = r;
          })
        }
        onDirty={vi.fn()}
      />,
    );
    result.unmount();
    await act(async () => resolve(catalog));
    expect(screen.queryByText("Employees")).toBeNull();
  });
  it("ignores failed key lookup after unmount", async () => {
    let reject!: (error: Error) => void;
    const result = render(
      <KeyPicker
        initial={[]}
        load={() =>
          new Promise((_, r) => {
            reject = r;
          })
        }
        elsewhere={() => new Set()}
        onApply={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    result.unmount();
    await act(async () => reject(new Error("offline")));
    expect(screen.queryByRole("dialog")).toBeNull();
  });
  it("supports a standalone field host without assuming a parent form", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    host.innerHTML =
      '<select name="endpoint_entra_mode"><option value="required">Required</option></select>';
    let dispose!: () => void;
    await act(async () => {
      dispose = mountIdentityEditor(host, async () => catalog);
    });
    fireEvent.change(
      screen.getByRole("textbox", { name: "Entra audience (required)" }),
      { target: { value: "api://standalone" } },
    );
    expect(
      (
        screen.getByRole("textbox", {
          name: "Entra audience (required)",
        }) as HTMLInputElement
      ).value,
    ).toBe("api://standalone");
    await act(async () => dispose());
    host.remove();
  });
});

describe("route configuration drawer", () => {
  it("moves the original bound form and preserves drafts and submit handlers", async () => {
    const { mountRouteConfiguration } =
      await import("../crates/gateway-api/admin-ui/src/react/route-configuration");
    const table = document.createElement("table");
    document.body.append(table);
    table.innerHTML =
      '<tbody><tr><td><code>/v1/responses</code></td><td><form><select name="mode"><option value="managed_by_gateway">managed_by_gateway</option></select><input name="timeout_ms" value="120000"><button>Save</button></form></td></tr></tbody>';
    const form = table.querySelector("form")!;
    const submit = vi.fn((event) => event.preventDefault());
    form.addEventListener("submit", submit);
    const drawer = document.createElement("div");
    document.body.append(drawer);
    let close!: () => void;
    let dispose!: () => void;
    await act(async () => {
      dispose = mountRouteConfiguration(form, (_title, node, onClose) => {
        drawer.append(node);
        close = onClose;
      });
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Configure /v1/responses" }),
    );
    expect(drawer.firstChild).toBe(form);
    fireEvent.change(form.querySelector("input")!, {
      target: { value: "80000" },
    });
    fireEvent.submit(form);
    expect(submit).toHaveBeenCalledOnce();
    close();
    expect(form.parentElement?.hidden).toBe(true);
    fireEvent.click(
      screen.getByRole("button", { name: "Configure /v1/responses" }),
    );
    expect(new FormData(form).get("timeout_ms")).toBe("80000");
    await act(async () => {
      table.remove();
    });
    dispose();
    drawer.remove();
  });
  it("supports service and unnamed route forms without modes", async () => {
    const { mountRouteConfiguration } =
      await import("../crates/gateway-api/admin-ui/src/react/route-configuration");
    for (const attr of [
      'data-service-name="ocr"',
      'data-route-id="models"',
      "",
    ]) {
      const host = document.createElement("div");
      document.body.append(host);
      host.innerHTML = `<form ${attr}></form>`;
      let dispose!: () => void;
      await act(async () => {
        dispose = mountRouteConfiguration(host.querySelector("form")!, vi.fn());
      });
      expect(host.querySelector("button")?.textContent).toBe("Configure");
      await act(async () => dispose());
      host.remove();
    }
  });
});

// Per-key route assignment workflow: actual profile wire shape and revision state.
import { KeyRouteBindings, mountKeyRouteBindings, type BindingEntry } from "../crates/gateway-api/admin-ui/src/react/key-route-bindings";
const bindingEntries = (): BindingEntry[] => [
  { route: "/v1/responses", url: "/routes", method: "PUT", access: { other_policy: "preserved", authentication_profiles: { revision: 3,
    profiles: [
      { id: "employees", name: "Employees", enabled: true, type: "entra_and_relayna_key" },
      { id: "automation", name: "Automation", enabled: true, type: "relayna_key_only" },
      { id: "disabled", name: "Disabled profile", enabled: false, type: "relayna_key_only" },
    ], bindings: [{ key_id: "other", profile_id: "employees" }] } } },
  { route: "/services/test", url: "/services/test", method: "PATCH", access: { authentication_profiles: { revision: 2,
    profiles: [{ id: "service", name: "Service profile", enabled: true, type: "relayna_key_only" }], bindings: [{ key_id: "this-key", profile_id: "service" }] } } },
  { route: "/v1/embeddings", url: "/routes", method: "PUT", access: {} },
];
function bindingsProps() {
  return { keyId: "this-key", keyName: "Production automation", entries: bindingEntries(), trigger: document.createElement("button"),
    onClose: vi.fn(), save: vi.fn(async (_entry, access) => ({ ...access, authentication_profiles: { ...access.authentication_profiles, revision: access.authentication_profiles.revision + 1 } })) };
}
describe("key route assignment dialog", () => {
  it("groups routes, searches paths, explains types and saves only the changed key with fresh revisions", async () => {
    const props = bindingsProps(); const user = userEvent.setup(); render(<KeyRouteBindings {...props} />);
    expect(screen.getByText("Production automation")).toBeTruthy();
    const row = within(screen.getByRole("group", { name: "/v1/responses" }));
    expect(row.getByRole("button", { name: "Save" }).disabled).toBe(true);
    expect(screen.getByText("Routes using existing settings (1)").parentElement.open).toBe(false);
    await user.click(screen.getByText("Routes using existing settings (1)"));
    await user.type(screen.getByLabelText("Find a route"), " NO MATCH ");
    expect(screen.getByText("No profile routes match your search.")).toBeTruthy();
    expect(screen.getByText("No routes match your search.")).toBeTruthy();
    await user.clear(screen.getByLabelText("Find a route"));
    const select = screen.getAllByLabelText("Authentication profile")[0];
    await user.selectOptions(select, "employees");
    expect(screen.getByText("After saving: Requires this Relayna key and a verified Entra identity.")).toBeTruthy();
    await user.selectOptions(select, "disabled");
    expect(screen.getByText(/After saving: This profile is disabled/)).toBeTruthy();
    await user.click(screen.getAllByRole("button", { name: "Save" })[0]);
    expect(screen.getByText("Profile disabled")).toBeTruthy();
    await user.selectOptions(select, "automation");
    await user.click(screen.getAllByRole("button", { name: "Save" })[0]);
    expect(props.save.mock.calls[1][1].authentication_profiles.revision).toBe(4);
    expect(props.save.mock.calls[1][1].other_policy).toBe("preserved");
    expect(props.save.mock.calls[1][1].authentication_profiles.bindings).toEqual([{key_id:"other",profile_id:"employees"},{key_id:"this-key",profile_id:"automation"}]);
    expect(screen.getAllByText("Requires this Relayna key only.")).toHaveLength(2);
    await user.selectOptions(select, "");
    expect(screen.getByText("After saving: This key cannot call this route.")).toBeTruthy();
    await user.click(screen.getAllByRole("button", { name: "Save" })[0]);
    expect(props.save.mock.calls[2][1].authentication_profiles.bindings).toEqual([{key_id:"other",profile_id:"employees"}]);
    expect(screen.getByRole("status").textContent).toBe("Saved");
    await user.click(screen.getByRole("button", { name: "Done" })); expect(props.onClose).toHaveBeenCalledOnce();
  });
  it("keeps failed drafts, explains conflicts, guards pending saves and supports retry", async () => {
    const props = bindingsProps(); let reject;
    props.save.mockImplementationOnce(() => new Promise((_resolve, fail) => { reject = fail; }));
    const user = userEvent.setup(); render(<KeyRouteBindings {...props} />);
    await user.selectOptions(screen.getAllByLabelText("Authentication profile")[0], "employees");
    await user.click(screen.getAllByRole("button", { name: "Save" })[0]);
    expect(screen.getByRole("button", { name: "Done" }).disabled).toBe(true);
    expect(screen.getAllByLabelText("Authentication profile").every(control => control.disabled)).toBe(true);
    await user.click(screen.getByRole("button", { name: "Close dialog" })); expect(props.onClose).not.toHaveBeenCalled();
    await act(async () => reject(new Error("authentication_profile_conflict: stale")));
    expect(screen.getByRole("alert").textContent).toContain("Close and reopen");
    expect(screen.getAllByLabelText("Authentication profile")[0].value).toBe("employees");
    props.save.mockRejectedValueOnce(new Error("Network unavailable"));
    await user.click(screen.getAllByRole("button", { name: "Save" })[0]);
    expect(screen.getByRole("alert").textContent).toContain("Network unavailable");
    await user.click(screen.getAllByRole("button", { name: "Save" })[0]);
    expect(screen.getByRole("status").textContent).toBe("Saved");
  });
  it("confirms discarding unsaved changes and keeps saved routes independent", async () => {
    const props = bindingsProps(); const user = userEvent.setup(); render(<KeyRouteBindings {...props} />);
    await user.selectOptions(screen.getAllByLabelText("Authentication profile")[1], "");
    await user.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.getByRole("alert").textContent).toContain("Assignments already saved will stay");
    await user.click(screen.getByRole("button", { name: "Keep editing" }));
    expect(screen.getAllByLabelText("Authentication profile")[1].value).toBe("");
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: "Discard changes" }));
    expect(props.onClose).toHaveBeenCalledOnce(); expect(props.save).not.toHaveBeenCalled();
  });
  it("shows setup guidance with no profile routes and mounts/cleans up the dialog", async () => {
    const props = bindingsProps(); props.entries = [props.entries[2]];
    let close;
    await act(async () => { close = mountKeyRouteBindings(props); });
    expect(screen.getByText(/No routes use profiles yet/)).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    props.entries = [];
    await act(async () => { close = mountKeyRouteBindings(props); });
    expect(screen.queryByText(/Routes using existing settings/)).toBeNull();
    await act(async () => close());
  });
});

import { FoundryEditor, mountFoundryEditor } from "../crates/gateway-api/admin-ui/src/react/foundry-editor";
describe("Foundry connection and service editors", () => {
  const props = () => ({kind:"provider" as const,providers:[],projects:[],onSave:vi.fn().mockResolvedValue(undefined),onClose:vi.fn()});
  const fill = (label, value) => fireEvent.change(screen.getByLabelText(label), {target:{value}});
  const submit = () => fireEvent.submit(screen.getByRole("button",{name:"Save"}).closest("form"));
  it("creates workload, managed and client-secret identities and retains errors for correction", async () => {
    const p=props(); const user=userEvent.setup(); const view=render(<FoundryEditor {...p}/>);
    fill("Name","Azure");fill("Project endpoint","https://account.services.ai.azure.com/api/projects/demo");fill("Tenant ID","tenant");fill("Client ID","client");
    await act(async()=>submit()); expect(p.onSave).toHaveBeenLastCalledWith(expect.objectContaining({provider:"azure-foundry",foundry:{method:"workload_identity",tenant_id:"tenant",client_id:"client"}}));
    view.unmount();
    const managed=props();render(<FoundryEditor {...managed}/>);
    await user.selectOptions(screen.getByLabelText("Azure identity"),"managed_identity");expect(screen.queryByLabelText("Tenant ID")).toBeNull();fill("Name","Managed");fill("Project endpoint","https://account.services.ai.azure.com/api/projects/demo");
    managed.onSave.mockRejectedValueOnce(new Error("Check the project endpoint."));await act(async()=>submit());expect(screen.getByRole("alert").textContent).toBe("Check the project endpoint.");
    expect(managed.onSave).toHaveBeenLastCalledWith(expect.objectContaining({foundry:{method:"managed_identity",tenant_id:null,client_id:null}}));
    await user.selectOptions(screen.getByLabelText("Azure identity"),"client_secret");fill("Tenant ID","tenant");fill("Client ID","client");fill("Client secret","keep spaces ");
    managed.onSave.mockRejectedValueOnce("Unavailable");await act(async()=>submit());expect(screen.getByRole("alert").textContent).toBe("Unavailable");
    await act(async()=>submit());expect(managed.onSave).toHaveBeenLastCalledWith(expect.objectContaining({credential:"keep spaces ",foundry:{method:"client_secret",tenant_id:"tenant",client_id:"client"}}));
    expect(managed.onClose).toHaveBeenCalledOnce();
  });
  it("edits connections without erasing their saved secret and prevents closing a pending save", async()=>{
    const p=props();let resolve;p.onSave.mockImplementationOnce(()=>new Promise(r=>{resolve=r}));const user=userEvent.setup();
    render(<FoundryEditor {...p} record={{id:"provider",name:"Existing",base_url:"https://account.services.ai.azure.com/api/projects/demo",credential_configured:true,foundry:{method:"client_secret",tenant_id:"tenant",client_id:"client"}}}/>);
    expect(screen.getByLabelText("Replace client secret (optional)").required).toBe(false);
    await act(async()=>submit());expect(screen.getByRole("button",{name:"Saving…"}).disabled).toBe(true);
    await user.click(screen.getByRole("button",{name:"Close dialog"}));expect(p.onClose).not.toHaveBeenCalled();
    expect(p.onSave.mock.calls[0][0]).not.toHaveProperty("credential");expect(p.onSave.mock.calls[0][0]).not.toHaveProperty("provider");
    await act(async()=>resolve());expect(p.onClose).toHaveBeenCalledOnce();
  });
  it("creates registered and passthrough services and explains missing connections",async()=>{
    const p={...props(),kind:"service" as const};const view=render(<FoundryEditor {...p}/>);
    expect(screen.getByRole("button",{name:"Save"}).disabled).toBe(true);expect(screen.getByRole("status").textContent).toContain("Providers first");view.unmount();
    const user=userEvent.setup();const next={...p,providers:[{id:"p1",name:"Production",enabled:true},{id:"p2",name:"Disabled",enabled:false}],projects:[{id:"project",name:"Research"}]};
    render(<FoundryEditor {...next}/>);fill("Name","research");await user.selectOptions(screen.getByLabelText("Foundry connection"),"p1");fill("Agent name","web-research");fill("Agent version (optional)","3");await user.selectOptions(screen.getByLabelText("Project (optional)"),"project");await user.selectOptions(screen.getByLabelText("Caller authentication"),"key_only");
    await act(async()=>submit());expect(p.onSave).toHaveBeenLastCalledWith(expect.objectContaining({name:"research",project_id:"project",access:{skip_entra:true},foundry:{mode:"registered_agent",provider_id:"p1",agent_name:"web-research",agent_version:"3"}}));
    await user.selectOptions(screen.getByLabelText("Integration mode"),"endpoint_passthrough");expect(screen.queryByLabelText("Agent name")).toBeNull();await user.selectOptions(screen.getByLabelText("Project (optional)"),"");await user.selectOptions(screen.getByLabelText("Caller authentication"),"inherit");
    await act(async()=>submit());expect(p.onSave).toHaveBeenLastCalledWith(expect.objectContaining({project_id:null,access:{skip_entra:false},foundry:{mode:"endpoint_passthrough",provider_id:"p1"}}));
  });
  it("edits an agent binding independently of route authentication and restores optional version",async()=>{
    const p={...props(),kind:"service" as const,providers:[{id:"p1",name:"Production",enabled:true}]};const user=userEvent.setup();
    render(<FoundryEditor {...p} record={{name:"research",foundry:{mode:"registered_agent",provider_id:"p1",agent_name:"agent",agent_version:"2"}}}/>);
    expect(screen.queryByLabelText("Name")).toBeNull();expect(screen.queryByLabelText("Caller authentication")).toBeNull();fill("Agent version (optional)","");await act(async()=>submit());expect(p.onSave).toHaveBeenLastCalledWith({foundry:{mode:"registered_agent",provider_id:"p1",agent_name:"agent",agent_version:null}});
    await user.click(screen.getByRole("button",{name:"Cancel"}));expect(p.onClose).toHaveBeenCalledTimes(2);
    await user.click(screen.getByRole("button",{name:"Close dialog"}));expect(p.onClose).toHaveBeenCalledTimes(3);
  });
  it("mounts and removes the standalone dialog",async()=>{
    const p=props();let close;await act(async()=>{close=mountFoundryEditor(p)});expect(screen.getByRole("dialog")).toBeTruthy();await act(async()=>close());expect(screen.queryByRole("dialog")).toBeNull();
  });
});

import { FoundryCheck, mountFoundryCheck, type FoundryCheckResult } from "../crates/gateway-api/admin-ui/src/react/foundry-check";
describe("Foundry connection verification", () => {
  const result: FoundryCheckResult = { provider_id:"p1", configuration_revision:1, checked_at:"2026-09-20T10:00:00Z", instance_id:"gateway-instance-1", identity_method:"workload_identity", identity:{status:"passed",code:"token_acquired",message:"Fresh Azure token acquired.",http_status:null}, project:{status:"passed",code:"project_read_verified",message:"Project read verified; invocation not tested.",http_status:200} };
  it("runs only on request, shows stages and instance, clears stale results on retry", async () => {
    const user=userEvent.setup(); const close=vi.fn(); const onCheck=vi.fn().mockResolvedValue(result);
    render(<FoundryCheck name="Production Foundry" onCheck={onCheck} onClose={close}/>);
    expect(onCheck).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button",{name:"Check connection"}));
    expect(await screen.findByText("Fresh Azure token acquired.")).toBeTruthy();
    expect(screen.getByText(/Gateway instance: gateway-instance-1/)).toBeTruthy();
    expect(screen.getByText("Verified · HTTP 200")).toBeTruthy();
    let resolve!: (r:FoundryCheckResult)=>void;
    onCheck.mockImplementationOnce(()=>new Promise(r=>{resolve=r;}));
    await user.click(screen.getByRole("button",{name:"Check again"}));
    expect(screen.queryByText("Fresh Azure token acquired.")).toBeNull();
    expect(screen.getByRole("button",{name:"Checking…"}).hasAttribute("disabled")).toBe(true);
    await user.keyboard("{Escape}"); expect(close).not.toHaveBeenCalled();
    await act(async()=>resolve({...result,project:{status:"inconclusive",code:"project_read_forbidden",message:"Read permission denied; invocation is unverified.",http_status:403}}));
    expect(screen.getByText("Not verified · HTTP 403")).toBeTruthy();
    await user.click(screen.getByRole("button",{name:"Close"}));expect(close).toHaveBeenCalledOnce();
  });
  it("keeps identity failure, skipped probe and request failure distinct",async()=>{
    const user=userEvent.setup();const close=vi.fn();
    const onCheck=vi.fn().mockResolvedValueOnce({...result,identity:{status:"failed",code:"workload_token_missing",message:"Configure the pod workload token.",http_status:null},project:{status:"skipped",code:"identity_required",message:"Fix identity and retry.",http_status:null}}).mockRejectedValueOnce(new Error("private diagnostic"));
    render(<FoundryCheck name="Connection" onCheck={onCheck} onClose={close}/>);
    await user.click(screen.getByRole("button",{name:"Check connection"}));
    expect(screen.getByText("Failed")).toBeTruthy(); expect(screen.getByText("Not checked")).toBeTruthy();
    await user.click(screen.getByRole("button",{name:"Check again"}));
    expect(screen.getByRole("alert").textContent).toContain("provider-management permission");expect(screen.queryByText("private diagnostic")).toBeNull();
    await user.keyboard("{Escape}");expect(close).toHaveBeenCalledOnce();
  });
  it("mounts and unmounts its own dialog",async()=>{
    await act(async()=>{mountFoundryCheck({name:"Connection",onCheck:vi.fn()});});
    await userEvent.setup().click(screen.getByRole("button",{name:"Close"}));expect(screen.queryByRole("dialog")).toBeNull();
  });
});
