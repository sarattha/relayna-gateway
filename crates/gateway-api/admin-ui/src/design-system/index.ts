import { metaForView } from "./view-meta";

export * from "./components";
export * from "./templates";
export * from "./view-meta";

export function applyViewChrome(view: string): void {
  const meta = metaForView(view);
  document.documentElement.dataset.viewDomain = meta.domain.toLowerCase();
  document.querySelector("#view-title")!.textContent = meta.title;
  const domain = document.querySelector("#view-domain");
  if (domain) domain.textContent = meta.domain;
  const summary = document.querySelector("#view-summary");
  if (summary) summary.textContent = meta.summary;
}
