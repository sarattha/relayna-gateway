# LiteLLM customer usage and costs

Introduced in **1.3.0**, the **LiteLLM customer costs** panel in **Monitor →
Usage & cost** reports USD cost and request count per customer ID for a Relayna
virtual key's mapped LiteLLM credential. It supports `managed_by_gateway`
requests, paginated browsing, exact-ID lookup and streaming CSV export.

## Where the data lives

**LiteLLM stores the customer directory and customer activity.** Gateway reads
that data on demand; this feature does not synchronize customer records or
customer-cost aggregates into Gateway's PostgreSQL database. Gateway retains
its existing credential mappings and ordinary request usage records separately.

The data path is browser → Gateway control API → LiteLLM customer directory and
activity APIs → browser. The browser keeps at most three result pages in memory
for 30 seconds. There is no persistent customer-report cache or background sync.
Refreshing reads LiteLLM again. If LiteLLM is unavailable, the report is
unavailable; Gateway does not substitute its own usage totals.

## Before you start

1. Configure an enabled LiteLLM provider and a key- or project-scoped
   [LiteLLM credential mapping](admin-portal.md#litellm-custom-header-and-virtual-key-mapping).
   A default provider credential alone does not establish the report's key mapping.
2. Ensure LiteLLM records customer IDs and daily activity. Use opaque customer
   identifiers, and register customers in LiteLLM for directory browsing/export.
   Gateway does not create customer registrations.
3. Give the configured provider credential permission to read LiteLLM's
   `/customer/list` and `/customer/daily/activity/aggregated` endpoints. These
   are server-side analytics reads; exposing the LiteLLM dashboard to callers
   is not required.
4. Sign in to Gateway with `usage:read`; add `usage:export` to download all rows.

## Browse customer costs

In **Usage & cost**, select a **Virtual key** and apply filters to show
**LiteLLM customer costs**. Choose **From (UTC day)** and **To (UTC day)**, then
select **Load customer costs**. The table shows each recorded **Customer ID**,
**Cost (USD)** and **Requests** for the selected key's mapped LiteLLM credential.
Enabled key mappings take precedence over project mappings, including for
`managed_by_gateway` requests.

The table defaults to **50 registered customers per page**, with a maximum of
100. Use **Previous customer page** and **Next customer page**, or enter an
exact **Customer ID** to look it up directly. Directory pages follow LiteLLM's
registered customer directory; only customers with activity for the mapped key
appear in the table. A directory page can have no matching activity and still
have a next page. Costs are sorted within each page, not across the directory.
The browser retains at most three recent pages for 30 seconds; **Load customer
costs** refreshes the data and returns to the first page.

The dates initially cover the Usage time range's UTC calendar days. **All time**
defaults to the last 30 UTC days; choose a range of up to 366 inclusive days.
These are whole-day LiteLLM totals, so partial days in the Usage time range are
expanded to whole UTC days. Other Usage filters, such as model, service and
status, do not apply to this table. Reload after changing dates to fetch new
totals. This date-scoped activity can differ from the current resettable key
counter shown by **View LiteLLM reported spend**.

LiteLLM must already record customer IDs and daily activity. Its configured
provider credential needs admin analytics permission for
`GET /customer/list` and `GET /customer/daily/activity/aggregated`. Gateway reads
one bounded directory page, then requests complete aggregated costs for only
those customer IDs, filtered by the mapped LiteLLM key's SHA-256 identifier.
Exact-ID lookup bypasses the directory. Gateway uses only the matching key's customer breakdown;
it does not substitute the deployment-wide `/customer/info` spend. Shared
credentials include other callers and traffic outside Gateway. Customer costs
are read-only and do not change Gateway usage records or budget enforcement.

Gateway never loads the complete directory into memory or requests unfiltered
customer analytics. A normal page needs at most two upstream reads; exact-ID lookup needs
one. Each read response is capped at 2 MiB, with a five-second deadline for the
whole page. LiteLLM versions without the aggregated endpoint or bounded customer
listing show **unavailable**. LiteLLM's own database controls query latency;
deep directory pages use upstream offset pagination. Exact-ID lookup is the
preferred way to reach a known customer in a very large directory. Customer IDs
must fit within 1,024 bytes and contain no commas or control characters because
LiteLLM's batch filter uses comma-separated IDs.

For example, send an opaque customer ID with a managed request:

```bash
curl "$GATEWAY_URL/v1/chat/completions" \
  -H "Authorization: Bearer $RELAYNA_API_KEY" \
  -H "Content-Type: application/json" \
  -H "x-litellm-customer-id: customer-123" \
  -d '{"model":"your-litellm-model","messages":[{"role":"user","content":"Hello"}]}'
```

LiteLLM also accepts the top-level JSON `user` field. Prefer opaque identifiers;
Gateway body guardrails such as PII redaction can transform email-like body
values. See [LiteLLM customer tracking](https://docs.litellm.ai/docs/proxy/customers)
for upstream configuration and customer ID rules. Requests without a customer ID
are excluded from the table. An available empty result says no activity was
recorded for the checked customers; failed reads display **unavailable**, without
showing zero costs. If activity exceeds the bounded read, try a shorter date
range or smaller page.

The additive endpoint is
`GET /admin-ui/admin/keys/{key_id}/litellm-customer-spend?start_date=YYYY-MM-DD&end_date=YYYY-MM-DD`.
It requires `usage:read`, returns `Cache-Control: no-store`, and returns 404 for
an unknown Gateway key. Dates are required and inclusive in UTC; invalid or
oversized ranges return 400. Optional `page` starts at 1, `page_size` defaults to
50 and accepts 1–100, and `customer_id` performs an exact lookup. The response
includes `key_id`, `status`, `mapping_scope`, `start_date`, `end_date`, `customers`,
`fetched_at`, `page`, `page_size`, `has_more`, `customers_checked` and `customer_id`.
Each customer has `customer_id`, `spend_usd` and `requests`. Status is `available`,
`not_mapped` or `unavailable`; only available results carry customer totals and
a retrieval timestamp. `has_more` describes directory pagination, independently
of how many matching cost rows are returned. A full final directory page can
have one empty page after it. Exact lookup returns page 1 with `has_more: false`.
Incomplete or unsupported upstream aggregates produce no partial totals.

## Export every customer's usage

Select **Export all customers (CSV)** to download every currently registered
customer with activity for the mapped credential in the selected UTC dates. This includes the customer
ID, USD spend, request count, date range and Gateway key ID. The current table
page, page size and exact-ID filter do not limit the export. Export requires
the `usage:export` scope.

Gateway walks the directory in batches of 100 and streams matching usage rows
as it reads each batch. It does not collect the complete directory or CSV in
memory, and there is no 1,000-customer export cap. At most two customer exports
run concurrently per Gateway process. Each batch has a five-second read
deadline; the overall download can take longer for a large directory. Customer
and cost data can change between batches: LiteLLM does not provide a directory
snapshot for this operation. Deleted customers that retain historical spend
are not discoverable in the directory; use exact-ID lookup to inspect their
remaining history.

In browsers with streaming file access, choose a destination and let the
download finish. **Cancel export**, navigation away, or an upstream failure
aborts the download without committing the partial file. If the browser does
not support streaming file access, use **Copy export curl** and set
`GATEWAY_OPERATOR_TOKEN` to an operator token with `usage:export`.

The endpoint is
`GET /admin-ui/admin/keys/{key_id}/litellm-customer-spend/export.csv?start_date=YYYY-MM-DD&end_date=YYYY-MM-DD`.
It returns a streamed CSV attachment with `Cache-Control: no-store`. Customer
IDs are CSV-escaped and spreadsheet formula prefixes are neutralized. A missing
mapping returns 409 and an initial upstream failure returns 502. Failures after
streaming starts interrupt the response; command-line consumers must check the
download's exit status and discard incomplete files.

## Large customer directories

Gateway bounds work by page rather than total registered customers. With one
million registered IDs, opening the report still checks only 50 IDs by default
(100 maximum) and renders at most that page. Known-ID lookup skips the directory
read. Export reads 100 IDs per batch and streams rows with backpressure, keeping
memory bounded rather than building a million-row response in memory.

These bounds do not guarantee a particular LiteLLM query time. Directory pages
use LiteLLM's offset pagination, so deep pages can be slower as its database
grows. Prefer exact-ID lookup for a known customer. A full export must traverse
the directory and can take substantially longer than browsing a page.

## Troubleshooting and attribution

| Result | What to check |
| --- | --- |
| No mapped credential | Enable a key or project mapping for the selected Relayna key. |
| Available, empty page | The checked registered customers have no matching activity for this credential and date range. Continue to the next page or use an exact ID. |
| Unavailable | Check LiteLLM reachability, analytics permissions, supported aggregate/list endpoints and the response/date bounds. Retry with fewer days or a smaller page. |
| Costs differ from Gateway Usage | LiteLLM uses whole UTC days and includes every caller sharing the mapped credential. Other Gateway Usage filters do not apply. |
| A historical customer is missing from export | Export discovers currently registered IDs. Use a known exact ID for a deleted customer's retained activity. |
| Export interrupted | Retry after resolving the upstream problem and discard incomplete command-line output. Use the UI's copied curl command with failed-download cleanup. |

A project mapping can be shared across Relayna keys, and a LiteLLM credential
can be used outside Gateway. Do not add reports for shared credentials together
as if they represented independent spend. Use dedicated LiteLLM credentials
when exclusive attribution is required. These reads do not alter Gateway
budgets, recorded request costs or LiteLLM customer registrations.
