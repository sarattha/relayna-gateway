import { html } from "./components";

export function workflowIcon(title: string): string {
  const name = title.toLowerCase();
  if (/simulator/.test(name)) return "test-pipe";
  if (/guardrail|safeguard/.test(name)) return "shield-check";
  if (/identity|authentication|entra/.test(name)) return "fingerprint";
  if (/policy|layer/.test(name)) return "stack-2";
  if (/pricing|cost|budget/.test(name)) return "coin";
  if (/key|credential/.test(name)) return "key";
  if (/project/.test(name)) return "folders";
  if (/provider|upstream|connection/.test(name)) return "server-2";
  if (/socket|transport|stream/.test(name)) return "plug-connected";
  if (/request|investigation|traffic/.test(name)) return "activity";
  if (/route|access|limit/.test(name)) return "route";
  if (/service|endpoint/.test(name)) return "cube";
  if (/data|usage/.test(name)) return "chart-bar";
  return "adjustments";
}

export function drawerHeading(title: string, closeControl: string, titleId: string): string {
  const readOnly = /investigation|chart data|request details/i.test(title);
  return `<div class="drawer-heading"><span class="drawer-mark" aria-hidden="true"><i class="ti ti-${workflowIcon(title)}"></i></span><div class="drawer-title"><span class="eyebrow">${readOnly ? "Inspect & analyze" : "Configure gateway"}</span><h3 id="${html(titleId)}">${html(title)}</h3></div>${closeControl}</div>`;
}

export function prepareDrawerActions(body: HTMLElement): void {
  for (const form of body.querySelectorAll<HTMLFormElement>("form")) {
    if (form.closest("[data-react-ui]") || form.querySelector(":scope > .form-actions")) continue;
    const buttons = [...form.querySelectorAll<HTMLButtonElement>(':scope > button[type="submit"], :scope > button:not([type])')];
    if (!buttons.length) continue;
    const actions = body.ownerDocument.createElement("div");
    actions.className = "form-actions wide-field";
    form.append(actions);
    actions.append(...buttons);
  }
}

// Navigation is a table of contents, not tabs: all original form controls stay
// mounted, retain their values and participate in the same form submission.
export function mountDrawerNavigation(dialog: HTMLElement): () => void {
  const body = dialog.querySelector<HTMLElement>(".drawer-body");
  if (!body) return () => {};
  const sections = [...body.querySelectorAll<HTMLDetailsElement>(".form-section")];
  if (sections.length < 2) return () => {};
  const doc = dialog.ownerDocument;
  const nav = doc.createElement("nav");
  nav.className = "drawer-nav";
  nav.setAttribute("aria-label", "Editor sections");
  const buttons = sections.map((section, index) => {
    const title = section.querySelector("summary strong")?.textContent || `Section ${index + 1}`;
    const button = doc.createElement("button");
    button.type = "button";
    button.innerHTML = `<i class="ti ti-${workflowIcon(title)}" aria-hidden="true"></i><span>${html(title)}</span>`;
    button.addEventListener("click", () => {
      section.open = true;
      body.scrollTo({ top: body.scrollTop + section.getBoundingClientRect().top - body.getBoundingClientRect().top - 16 });
      section.querySelector<HTMLElement>("summary")?.focus({ preventScroll: true });
      buttons.forEach(item => item.removeAttribute("aria-current"));
      button.setAttribute("aria-current", "location");
    });
    nav.append(button);
    return button;
  });
  buttons[0].setAttribute("aria-current", "location");
  const update = () => {
    const top = body.getBoundingClientRect().top + 60;
    const current = sections.reduce((index, section, next) => section.getBoundingClientRect().top <= top ? next : index, 0);
    buttons.forEach((button, index) => {
      if (index === current) button.setAttribute("aria-current", "location");
      else button.removeAttribute("aria-current");
    });
  };
  body.addEventListener("scroll", update, { passive: true });
  body.before(nav);
  return () => { body.removeEventListener("scroll", update); nav.remove(); };
}
