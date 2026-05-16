# Guardrails

Guardrails let operators apply policy checks and transformations to
OpenAI-compatible JSON traffic before, after, or during provider calls. The
catalog defines which guardrails exist and their global defaults. Virtual key
policy decides which catalog entries apply to each key.

`pii-redact` is seeded as an enabled built-in guardrail. It is not default-on,
so existing keys keep current behavior until an operator selects it for a key.

## Concepts

| Concept | Purpose |
| --- | --- |
| Catalog definition | Global guardrail metadata, modes, failure policy, schema, runtime config, and HTTP provider settings. |
| Mandatory guardrails | Always run for a virtual key. Clients cannot disable them. |
| Optional guardrails | Allowed for a virtual key when clients request them. |
| Forbidden guardrails | Hidden from discovery and rejected if requested or configured as an override. |
| Runtime config | Global default JSON object passed to the guardrail when it runs. |
| Per-key override | Key-specific JSON object shallow-merged over runtime config for one guardrail. |

Effective config is calculated per applied guardrail:

```text
effective_config = catalog runtime_config + key guardrail_config_overrides[name]
```

Overrides must be JSON objects. They are dormant until the guardrail is applied
by mandatory, optional, default-on, or client-requested policy. Unknown
guardrail names, forbidden guardrail names, and scalar or array override values
are rejected with stable guardrail error envelopes.

## Set Up the Catalog

1. Start Gateway and sign in to Admin portal with the operator token.
2. Open Guardrails.
3. Select `pii-redact` or click `New guardrail` for a custom HTTP guardrail.
4. Configure global catalog fields:

   | Field | Notes |
   | --- | --- |
   | Name | Immutable after creation. |
   | Modes | `pre_call`, `post_call`, and optional `during_call`. |
   | Failure policy | `fail_closed`, `fail_open`, or `dry_run`. |
   | Enabled | Disabled guardrails cannot be applied. |
   | Default on | Applies the guardrail by default when allowed. Keep off for opt-in rollout. |
   | Config schema | Operator-facing schema or example for runtime config. |
   | Runtime config | Actual global default config used during execution. |
   | Endpoint URL | Custom HTTP guardrails only. |
   | Timeout ms | Custom HTTP guardrails only. |
   | Bearer token | Custom HTTP guardrails only, write-only. |

Built-in guardrails protect provider-specific fields: endpoint URL, bearer
token, provider kind, and delete are unavailable. Custom HTTP guardrails can be
created, edited, and deleted. Deleting a custom guardrail removes its name from
current key policy arrays and override maps but keeps historical execution
events.

## Configure Each Key

Open Keys in Admin portal and use the guardrail pickers:

1. Add guardrails that must always run to Mandatory guardrails.
2. Add guardrails clients may request to Optional guardrails.
3. Add guardrails clients must never use to Forbidden guardrails.
4. Set per-key config overrides after selecting a mandatory or optional
   guardrail. The portal hides override editors for unselected guardrails.

API shape:

```json
{
  "guardrail_policy": {
    "mandatory_guardrails": ["pii-redact"],
    "optional_guardrails": ["custom-check"],
    "forbidden_guardrails": ["debug-only"],
    "guardrail_config_overrides": {
      "pii-redact": {
        "restore_output": false
      },
      "custom-check": {
        "threshold": 0.85
      }
    }
  }
}
```

The `pii-redact` override above affects only this key. Other keys continue to
use the catalog `runtime_config` unless they define their own override.

## Built-In `pii-redact`

`pii-redact` scans JSON string leaves for common email, phone-like, and
SSN-like values.

- Pre-call replaces detected values with request-local placeholders such as
  `[EMAIL_1]`.
- Post-call can restore known placeholders when `restore_output` is true, then
  redacts newly generated PII.
- During-call support redacts streamed response chunks with a small holdback
  window for values split across chunks.
- Execution records include counts and categories only, not raw PII or body
  content.

Global runtime config example:

```json
{
  "restore_output": true
}
```

Per-key override example:

```json
{
  "restore_output": false
}
```

Use the per-key override when one application should keep placeholders redacted
while another application should receive restored values.

## Test Guardrails

Virtual keys can discover allowed guardrails:

```bash
curl -sS \
  -H "Authorization: Bearer rk_live_xxx" \
  http://127.0.0.1:8081/v1/guardrails
```

Use the test endpoint to run guardrails without calling a provider:

```bash
curl -sS \
  -H "Authorization: Bearer rk_live_xxx" \
  -H "Content-Type: application/json" \
  -X POST http://127.0.0.1:8081/v1/guardrails/test \
  -d '{"guardrails":["pii-redact"],"mode":"pre_call","input":{"messages":[{"role":"user","content":"email alice@example.com"}]}}'
```

Proxy traffic uses the same effective config as the test endpoint. Guarded
streaming requests require selected response guardrails to support
`during_call`; otherwise Gateway fails closed with `guardrail_unavailable`.
