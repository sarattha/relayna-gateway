import { emptyState, html, panel, tableWrap } from "./components";

export function dashboardTemplate(metrics: string, sections: string): string {
  return `<div class="grid stats">${metrics}</div>${sections}`;
}

export function listDetailTemplate(title: string, listMarkup: string, detailMarkup = ""): string {
  return `${panel(title, listMarkup)}${detailMarkup}`;
}

export function createEditTemplate(title: string, formMarkup: string): string {
  return panel(title, formMarkup);
}

export function filterPanel(title: string, formMarkup: string): string {
  return panel(title, `<form class="form-grid" data-filter-form>${formMarkup}</form>`);
}

export function auditLogTemplate(filterMarkup: string, tableMarkup: string): string {
  return `${filterPanel("Audit filters", filterMarkup)}${panel("Audit events", tableMarkup)}`;
}

export function analyticsTemplate(summaryMarkup: string, breakdownMarkup: string): string {
  return `<div class="grid stats">${summaryMarkup}</div>${breakdownMarkup}`;
}

export function importDiffTemplate(diff: Record<string, unknown[]>): string {
  const groups = [
    ["Added", diff.added],
    ["Changed", diff.changed],
    ["Removed", diff.removed],
    ["Invalid", diff.invalid],
  ];
  return `<div class="grid">${groups
    .map(([label, rows]) => {
      const items = Array.isArray(rows) && rows.length
        ? tableWrap(`<table><tbody>${rows.map((row) => `<tr><td>${html(nameForDiffRow(row))}</td><td>${html(reasonForDiffRow(row))}</td></tr>`).join("")}</tbody></table>`)
        : emptyState(`No ${String(label).toLowerCase()} services.`);
      return panel(String(label), items, Array.isArray(rows) ? `${rows.length} total` : "0 total");
    })
    .join("")}</div>`;
}

function nameForDiffRow(row: unknown): string {
  if (!row || typeof row !== "object") return String(row ?? "");
  const value = row as Record<string, unknown>;
  return String(value.name ?? value.service_name ?? value.id ?? value.studio_service_id ?? "service");
}

function reasonForDiffRow(row: unknown): string {
  if (!row || typeof row !== "object") return "";
  const value = row as Record<string, unknown>;
  return String(value.reason ?? value.change_type ?? value.status ?? "");
}
