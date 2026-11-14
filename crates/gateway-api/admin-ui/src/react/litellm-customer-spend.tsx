import { useEffect, useId, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { Button, Field, Input, NativeSelect } from "../components/ui";

type DateRange = { start_date: string; end_date: string };
type CustomerSpend = {
  key_id: string;
  status: "available" | "not_mapped" | "unavailable";
  mapping_scope: "key" | "project" | null;
  customers: { customer_id: string; spend_usd: number; requests: number }[];
  start_date: string;
  end_date: string;
  fetched_at: string | null;
  page: number;
  page_size: number;
  has_more: boolean;
  customers_checked: number;
  customer_id: string | null;
};
type Props = {
  keyId: string;
  from?: string | null;
  to?: string | null;
  api: (path: string, options: { signal: AbortSignal }) => Promise<CustomerSpend>;
  onExport: (dates: DateRange, signal: AbortSignal) => Promise<void>;
  onCopyExport: (dates: DateRange) => Promise<void>;
};

export function customerSpendDates(from?: string | null, to?: string | null): DateRange {
  const end = to ? new Date(to) : new Date();
  const start = from ? new Date(from) : new Date(end);
  if (!from) start.setUTCDate(start.getUTCDate() - 29);
  return { start_date: start.toISOString().slice(0, 10), end_date: end.toISOString().slice(0, 10) };
}

const usd = new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", minimumFractionDigits: 4, maximumFractionDigits: 8 });

export function LiteLlmCustomerSpend({ keyId, from, to, api, onExport, onCopyExport }: Props) {
  const titleId = useId();
  const hintId = useId();
  const searchHintId = useId();
  const [dates, setDates] = useState(() => customerSpendDates(from, to));
  const [customerId, setCustomerId] = useState("");
  const [pageSize, setPageSize] = useState("50");
  const [snapshot, setSnapshot] = useState<CustomerSpend | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [exporting, setExporting] = useState(false);
  const [exportMessage, setExportMessage] = useState("");
  const controller = useRef<AbortController | null>(null);
  const exportController = useRef<AbortController | null>(null);
  const pages = useRef(new Map<string, { snapshot: CustomerSpend; at: number }>());
  useEffect(() => () => { controller.current?.abort(); exportController.current?.abort(); }, []);
  const clearResults = () => { pages.current.clear(); setSnapshot(null); setError(""); };
  const changeDate = (field: keyof DateRange, value: string) => {
    setDates(previous => ({ ...previous, [field]: value }));
    clearResults();
  };
  const validDates = () => {
    const days = (Date.parse(dates.end_date) - Date.parse(dates.start_date)) / 86400000 + 1;
    if (!Number.isFinite(days) || days < 1 || days > 366) {
      setError("Choose a start and end date covering 1–366 inclusive UTC days.");
      return false;
    }
    return true;
  };
  const load = async (page = 1, refresh = false) => {
    if (!validDates()) return;
    const exactId = customerId.trim();
    if (/[\x00-\x1f\x7f,]/.test(exactId) || new TextEncoder().encode(exactId).length > 1024) {
      setError("Use an exact customer ID up to 1,024 bytes, without commas or control characters.");
      return;
    }
    const query = new URLSearchParams({ ...dates, page: String(page), page_size: pageSize });
    if (exactId) query.set("customer_id", exactId);
    const path = `/admin-ui/admin/keys/${encodeURIComponent(keyId)}/litellm-customer-spend?${query}`;
    if (refresh) pages.current.clear();
    const cached = pages.current.get(path);
    if (cached && Date.now() - cached.at < 30000) { setSnapshot(cached.snapshot); setError(""); return; }
    const request = new AbortController();
    controller.current?.abort();
    controller.current = request;
    setBusy(true); setError("");
    try {
      const result = await api(path, { signal: request.signal });
      if (request.signal.aborted) return;
      setSnapshot(result);
      if (result.status === "available") {
        pages.current.delete(path);
        pages.current.set(path, { snapshot: result, at: Date.now() });
        while (pages.current.size > 3) pages.current.delete(pages.current.keys().next().value!);
      }
    } catch {
      if (!request.signal.aborted) {
        setSnapshot(null);
        setError("Customer costs unavailable. Check the LiteLLM connection and permission to read the customer directory and aggregated daily activity, then retry.");
      }
    } finally {
      if (!request.signal.aborted) setBusy(false);
    }
  };
  const exportAll = async () => {
    if (!validDates()) return;
    const request = new AbortController();
    exportController.current = request;
    setExporting(true); setExportMessage(`Exporting all customers for ${dates.start_date} through ${dates.end_date} (UTC)…`);
    try {
      await onExport({ ...dates }, request.signal);
      if (!request.signal.aborted) setExportMessage("All-customer CSV saved.");
    } catch (error) {
      if (!request.signal.aborted) setExportMessage(error instanceof DOMException && error.name === "AbortError" ? "Export cancelled." : "Export failed. No completed file was saved. Retry, or use Copy export curl if this browser cannot stream files to disk.");
    } finally {
      if (!request.signal.aborted) setExporting(false);
    }
  };
  const cancelExport = () => { exportController.current?.abort(); setExporting(false); setExportMessage("Export cancelled."); };
  const copyExport = async () => {
    if (!validDates()) return;
    try { await onCopyExport({ ...dates }); setExportMessage("Export curl copied. Set GATEWAY_OPERATOR_TOKEN to a token with usage:export before running it."); }
    catch { setExportMessage("Could not copy the export command. Check clipboard permission and retry."); }
  };
  return <section className="breakdown-section" aria-labelledby={titleId}>
    <div className="panel-heading"><h4 id={titleId}>LiteLLM customer costs</h4></div>
    <p id={hintId} className="help">Customer totals for the selected key’s LiteLLM credential, including traffic outside Gateway. Dates include whole UTC days; other Usage filters do not apply. Shared mappings include every caller using that credential. These totals are separate from Gateway estimates and budgets.</p>
    <form id="litellm-customer-spend-form" className="form-grid" onSubmit={event => { event.preventDefault(); void load(1, true); }} aria-describedby={hintId}>
      <Field label="From (UTC day)"><Input type="date" required value={dates.start_date} disabled={busy} onChange={event => changeDate("start_date", event.target.value)} /></Field>
      <Field label="To (UTC day)"><Input type="date" required value={dates.end_date} disabled={busy} onChange={event => changeDate("end_date", event.target.value)} /></Field>
      <Field label="Customer ID (exact)"><Input type="search" value={customerId} disabled={busy} aria-describedby={searchHintId} onChange={event => { setCustomerId(event.target.value); clearResults(); }} /></Field>
      <Field label="Customers per page"><NativeSelect value={pageSize} disabled={busy} onChange={event => { setPageSize(event.target.value); clearResults(); }}><option value="25">25</option><option value="50">50</option><option value="100">100</option></NativeSelect></Field>
      <p id={searchHintId} className="help wide-field">Leave Customer ID blank to browse registered-customer pages. Only customers with activity for the mapped credential appear; a page can be empty and still have a next page. Enter an exact ID to look it up directly.</p>
      <div className="form-actions wide-field"><Button type="submit" disabled={busy}>{busy ? "Loading customer costs…" : "Load customer costs"}</Button></div>
    </form>
    <div className="form-actions">
      <Button disabled={exporting} onClick={() => void exportAll()}>Export all customers (CSV)</Button>
      {exporting && <Button onClick={cancelExport}>Cancel export</Button>}
      <Button onClick={() => void copyExport()}>Copy export curl</Button>
    </div>
    <p className="help">Export includes every currently registered customer with activity for this mapped credential in the selected UTC dates. The customer-ID filter and page size do not limit it. Large exports stream to disk in supported browsers.</p>
    <p role="status" aria-live="polite">{exportMessage}</p>
    <div role="status" aria-live="polite" aria-busy={busy}>
      {!snapshot && !busy && !error && <p className="help">Load recorded customers for these dates (up to 366 days). All time defaults to the last 30 UTC days.</p>}
      {error && <p>{error}</p>}
      {snapshot?.status === "not_mapped" && <p>No enabled LiteLLM credential mapping. Configure a key or project mapping in Providers.</p>}
      {snapshot?.status === "unavailable" && <p>Customer costs unavailable. Check LiteLLM customer directory and aggregated daily activity access, try a shorter date range or smaller page, then reload.</p>}
      {snapshot?.status === "available" && <>
        <p className="help">{snapshot.mapping_scope === "project" ? "Shared project mapping" : "Key mapping"} · {snapshot.start_date} through {snapshot.end_date} (UTC){snapshot.fetched_at && ` · Retrieved ${new Date(snapshot.fetched_at).toLocaleString()}`}</p>
        <p className="help">{snapshot.customer_id ? `Exact customer ID: ${snapshot.customer_id}` : `Directory page ${snapshot.page} · ${snapshot.customers_checked} registered IDs checked · ${snapshot.customers.length} customers with activity. Costs are sorted within this page.`}</p>
        {snapshot.customers.length ? <div className="table-wrap"><table>
          <caption className="sr-only">LiteLLM cost per customer for the mapped credential and selected UTC days</caption>
          <thead><tr><th scope="col">Customer ID</th><th scope="col">Cost (USD)</th><th scope="col">Requests</th></tr></thead>
          <tbody>{snapshot.customers.map(customer => <tr key={customer.customer_id}>
            <th scope="row"><code>{customer.customer_id}</code></th><td>{usd.format(customer.spend_usd)}</td><td>{customer.requests.toLocaleString()}</td>
          </tr>)}</tbody>
        </table></div> : <p>No activity recorded for {snapshot.customer_id ? "this customer" : "the customers checked on this directory page"} under the mapped credential in these UTC dates.{snapshot.has_more ? " Continue to the next directory page or look up an exact customer ID." : ""}</p>}
        {!snapshot.customer_id && <div className="form-actions" role="group" aria-label="Customer directory pagination">
          <Button disabled={busy || snapshot.page <= 1} onClick={() => void load(snapshot.page - 1)}>Previous customer page</Button>
          <Button disabled={busy || !snapshot.has_more} onClick={() => void load(snapshot.page + 1)}>Next customer page</Button>
        </div>}
      </>}
    </div>
  </section>;
}

export function mountLiteLlmCustomerSpend(host: HTMLElement, props: Props) {
  const root = createRoot(host);
  root.render(<LiteLlmCustomerSpend {...props} />);
  let closed = false;
  return () => { if (!closed) { closed = true; root.unmount(); } };
}
