# Relayna Admin UI 3.0 design package

Open `index.html` directly in Chrome. Fonts, chart library, styles and interactions are embedded. Data is synthetic; changes reset on reload. No real credentials are issued or stored.

For a local URL, run from repository root:

    python3 internal/design/admin-ui-3/serve.py

Open http://127.0.0.1:18430/ for UI 3. Open http://127.0.0.1:18430/admin-ui/ for unchanged UI 2 with sample responses. This fixture supports the audit's representative Overview, Keys, Usage, Providers and Health workflows; it is not a complete gateway simulator. Add `degraded.flag` in this directory to make readiness return 503; remove it to recover. `slow.flag` delays project responses for experiments.

`audit.md` contains 30 prioritized findings, all 18 existing surfaces, duplication mapping and an implementation sequence. `qa.md` documents prototype verification.

The embedded Chart.js distribution retains its license header. Icons use the repository's existing Tabler font. Production implementation must use the Vite/TypeScript source package and required verification workflow.
