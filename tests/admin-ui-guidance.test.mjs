import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";
const compile = path => ts.transpileModule(readFileSync(new URL(path, import.meta.url), "utf8"), {
  compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 },
}).outputText;
const moduleUrl = source => `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
const contentUrl = moduleUrl(compile("../crates/gateway-api/admin-ui/src/design-system/guidance-content.ts"));
const { fieldGuidance } = await import(contentUrl);
const guidance = compile("../crates/gateway-api/admin-ui/src/design-system/guidance.ts")
  .replace('"./guidance-content"', JSON.stringify(contentUrl));
const { tooltipPosition } = await import(moduleUrl(guidance));
// The same number has different operational meanings. Preserve the distinctions.
assert.match(fieldGuidance("rpm_limit", "policy"), /0 blocks requests/);
assert.match(fieldGuidance("daily_budget_usd", "policy"), /0 blocks immediately/);
assert.match(fieldGuidance("max_cost_per_request", "policy"), /0 allows only zero-cost estimates/);
assert.match(fieldGuidance("min_cost_usd", "usage"), /0 includes zero-cost/);
assert.match(fieldGuidance("max_request_body_bytes", "policy"), /inherited limits still apply/);
assert.match(fieldGuidance("max_request_body_bytes", "configuration"), /greater than 0/);
assert.match(fieldGuidance("from", "traffic"), /last 24 hours/);
assert.match(fieldGuidance("from", "usage"), /no lower bound/);
assert.match(fieldGuidance("request_id", "traffic"), /saved history matches the exact ID/);
assert.match(fieldGuidance("guardrail_override_names", "policy"), /Unchecking removes/);
assert.match(fieldGuidance("guardrail_override_pii", "policy"), /JSON object/);
assert.equal(fieldGuidance("unknown_limit", "policy"), undefined, "do not invent unknown field semantics");
// Placement remains visible near the right/bottom edges and prefers below when possible.
assert.deepEqual(tooltipPosition({left:30,top:20,bottom:44},{width:200,height:70},{width:1000,height:800}),{left:30,top:52});
assert.deepEqual(tooltipPosition({left:360,top:740,bottom:764},{width:320,height:100},{width:390,height:800}),{left:62,top:632});
assert.deepEqual(tooltipPosition({left:-20,top:5,bottom:29},{width:304,height:180},{width:320,height:200}),{left:8,top:8});
console.log("ok - guidance preserves contextual zero/blank semantics and clamps tooltip placement");
