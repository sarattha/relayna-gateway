import { useId, useState } from "react";
import { createRoot } from "react-dom/client";
import { flushSync } from "react-dom";
import { CheckboxField, Field, Input } from "../components/ui";

type Configuration = { enabled: boolean; header: string } | null;

function headerError(value: string) {
  const name = value.trim().toLowerCase();
  if (!name || name.length > 128 || !/^[!#$%&'*+.^_`|~0-9a-z-]+$/.test(name))
    return "User token header must be a valid HTTP header name, up to 128 characters.";
  if (/^(content-|accept-|if-|sec-|access-control-|proxy-|x-caller-|x-route-|x-relayna-|x-apigee-|x-forwarded-|x-original-|x-envoy-|x-ms-)/.test(name)
      || ["authorization", "proxy-authorization", "host", "connection", "keep-alive", "upgrade", "te", "trailer", "transfer-encoding", "cookie", "set-cookie", "origin", "referer", "forwarded", "via", "range", "expect", "cache-control", "pragma", "date", "location", "server", "vary", "allow", "etag", "last-modified", "traceparent", "tracestate", "baggage", "x-api-key", "api-key", "x-request-id", "x-correlation-id", "x-amzn-trace-id", "accept"].includes(name))
    return "User token header cannot replace authentication, trusted identity or HTTP control headers.";
  return "";
}

export function UserTokenForwarding({ initial }: { initial: Configuration }) {
  const [enabled, setEnabled] = useState(initial?.enabled ?? false);
  const [error, setError] = useState("");
  const helpId = useId();
  return <div className="grid gap-3" data-react-ui="">
    {initial && <input type="hidden" name="user_token_configured" value="true" />}
    <CheckboxField name="forward_user_token" label="Forward verified user token"
      help="Opt in only for a trusted Accessa adapter that needs the user JWT for a Keystone on-behalf-of exchange. Signed Apigee identity alone cannot be forwarded."
      checked={enabled} onChange={event => setEnabled(event.target.checked)} />
    <Field label="User token header">
      <Input name="user_token_header" defaultValue={initial?.header ?? "x-access-token"}
        maxLength={128} required aria-describedby={helpId} aria-invalid={Boolean(error)}
        onChange={event => {
          const message = headerError(event.target.value);
          event.target.setCustomValidity(message);
          setError(message);
        }} />
    </Field>
    <p id={helpId} className="help">Default: x-access-token. Gateway overwrites this header with the verified JWT when enabled and strips caller copies when disabled. Authentication, trusted identity and HTTP control headers are reserved. Tokens stay out of Traffic and Usage.</p>
    {error && <p role="alert" className="notice error">{error}</p>}
  </div>;
}

export function mountUserTokenForwarding(host: HTMLElement) {
  const initial = JSON.parse(host.dataset.userTokenForwarding || "null") as Configuration;
  const root = createRoot(host);
  flushSync(() => root.render(<UserTokenForwarding initial={initial} />));
  const observer = new MutationObserver(() => {
    if (!host.isConnected) { observer.disconnect(); root.unmount(); }
  });
  observer.observe(document.body, { childList: true, subtree: true });
}
