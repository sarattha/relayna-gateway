import { Button, Input, NativeSelect } from "../components/ui";

/** Controller viewport children are deliberately outside React reconciliation. */
export function ConsoleShell() {
  return (
    <>
      <a className="skip-link" href="#content">
        Skip to main content
      </a>
      <div id="login" className="login-shell">
        <section
          className="login-brand-panel"
          aria-label="Relayna Gateway overview"
        >
          <div className="login-brand-lockup">
            <span className="login-brand-icon" aria-hidden="true">
              <i className="ti ti-route-alt-left"></i>
            </span>
            <span>
              <strong>Relayna</strong>
              <small>Gateway</small>
            </span>
          </div>
          <div className="login-brand-message">
            <span className="login-brand-eyebrow">Internal AI gateway</span>
            <h1>
              Governed access.
              <br />
              Operational clarity.
            </h1>
            <p>
              One secure control plane for AI service traffic, usage, incidents,
              and service ownership.
            </p>
            <ul
              className="login-capabilities"
              aria-label="Gateway capabilities"
            >
              <li>
                <i className="ti ti-shield-check" aria-hidden="true"></i>Work
                account sign-in
              </li>
              <li>
                <i className="ti ti-chart-histogram" aria-hidden="true"></i>
                Usage visibility
              </li>
              <li>
                <i className="ti ti-key" aria-hidden="true"></i>Key governance
              </li>
              <li>
                <i className="ti ti-lock-access" aria-hidden="true"></i>Scoped
                access
              </li>
            </ul>
          </div>
          <small className="login-copyright">
            Relayna Gateway · Internal use only
          </small>
        </section>
        <section className="login-access-panel" aria-labelledby="login-title">
          <div className="login-access-card">
            <span className="login-access-eyebrow">Organization access</span>
            <h1 id="login-title">Welcome back</h1>
            <p className="login-copy">
              Sign in with your work or school account. Relayna will resolve
              your administrator role and service or project memberships after
              Microsoft verifies your identity.
            </p>
            <a
              id="entra-sign-in"
              className="login-primary"
              href="/admin-ui/auth/login"
            >
              <img
                src="/admin-ui/microsoft-sign-in.svg"
                width="215"
                height="41"
                alt="Sign in with Microsoft"
              />
            </a>
            <p id="oidc-unavailable" className="subtle hidden">
              Microsoft sign-in is not configured for this gateway.
            </p>
            <div className="login-session-divider">
              <span>Secure browser session</span>
            </div>
            <div className="login-security-note">
              <i className="ti ti-shield-lock" aria-hidden="true"></i>
              <span>
                <strong>Your identity tokens stay server-side</strong>
                <small>
                  The browser receives an HttpOnly session cookie. Gateway
                  permissions come from Relayna roles and exact resource
                  memberships.
                </small>
              </span>
            </div>
            <details className="break-glass-login">
              <summary>Emergency operator access</summary>
              <form id="login-form">
                <p className="help">
                  Use the break-glass operator token only when Microsoft sign-in
                  is unavailable.
                </p>
                <label>
                  Operator token
                  <Input
                    id="operator-token"
                    type="password"
                    autoComplete="current-password"
                  />
                </label>
                <Button type="submit">Use operator token</Button>
              </form>
            </details>
            <p id="login-error" className="error-text"></p>
            <Button
              id="access-state-sign-out"
              className="login-access-sign-out hidden"
              type="button"
            >
              Sign out and switch account
            </Button>
          </div>
        </section>
      </div>

      <main id="app" className="app-shell hidden">
        <Button
          id="nav-backdrop"
          className="nav-backdrop hidden"
          type="button"
          aria-label="Close navigation"
        ></Button>
        <aside id="sidebar" className="sidebar" aria-label="Admin navigation">
          <div className="sidebar-header">
            <div className="brand">
              <i className="ti ti-route" aria-hidden="true"></i>
              <span className="brand-title">
                <strong>relayna</strong>
                <small>Gateway console · 4.0</small>
                <span
                  className="version-indicator"
                  aria-label="Current Relayna Gateway version"
                >
                  v0.1.38
                </span>
              </span>
            </div>
            <Button
              id="nav-close"
              className="icon-button nav-close"
              type="button"
              aria-label="Close navigation"
            >
              <i className="ti ti-x" aria-hidden="true"></i>
            </Button>
          </div>
          <nav className="nav-groups" aria-label="Portal sections">
            <section className="nav-group" data-admin-nav>
              <span>Monitor</span>
              <Button data-view="overview" className="nav active">
                <i className="ti ti-layout-dashboard" aria-hidden="true"></i>
                <span>Overview</span>
              </Button>
              <Button data-view="traffic" className="nav">
                <i className="ti ti-activity" aria-hidden="true"></i>
                <span>Traffic</span>
              </Button>
              <Button data-view="usage" className="nav">
                <i className="ti ti-chart-line" aria-hidden="true"></i>
                <span>Usage &amp; cost</span>
              </Button>
              <Button data-view="health" className="nav">
                <i className="ti ti-heart-rate-monitor" aria-hidden="true"></i>
                <span>Health</span>
              </Button>
            </section>
            <section className="nav-group" data-admin-nav>
              <span>Discover</span>
              <Button data-view="projects" className="nav">
                <i className="ti ti-folders" aria-hidden="true"></i>
                <span>Projects</span>
              </Button>
              <Button data-view="services" className="nav">
                <i className="ti ti-cube" aria-hidden="true"></i>
                <span>Services</span>
              </Button>
              <Button data-view="providers" className="nav">
                <i className="ti ti-database" aria-hidden="true"></i>
                <span>Providers</span>
              </Button>
              <Button data-view="routes" className="nav">
                <i className="ti ti-route" aria-hidden="true"></i>
                <span>Routes</span>
              </Button>
            </section>
            <section className="nav-group" data-admin-nav>
              <span>Govern</span>
              <Button data-view="keys" className="nav">
                <i className="ti ti-key" aria-hidden="true"></i>
                <span>Virtual keys</span>
              </Button>
              <Button data-view="guardrails" className="nav">
                <i className="ti ti-shield-check" aria-hidden="true"></i>
                <span>Policies &amp; guardrails</span>
              </Button>
              <Button data-view="members" className="nav">
                <i className="ti ti-users" aria-hidden="true"></i>
                <span>People &amp; identities</span>
              </Button>
              <Button data-view="audit" className="nav">
                <i className="ti ti-file-search" aria-hidden="true"></i>
                <span>Audit log</span>
              </Button>
              <Button data-view="settings" className="nav">
                <i className="ti ti-settings" aria-hidden="true"></i>
                <span>Settings</span>
              </Button>
              <Button data-view="managed-identities" className="nav" hidden>
                <i className="ti ti-id-badge-2" aria-hidden="true"></i>
                <span>Managed identities</span>
              </Button>
            </section>
            <section className="nav-group" data-owner-nav>
              <span>Owner</span>
              <Button data-view="my-services" className="nav">
                <i className="ti ti-cubes" aria-hidden="true"></i>
                <span>My services</span>
              </Button>
              <Button data-view="service-dashboard" className="nav">
                <i className="ti ti-chart-histogram" aria-hidden="true"></i>
                <span>Service dashboard</span>
              </Button>
              <Button data-view="my-projects" className="nav">
                <i className="ti ti-folders" aria-hidden="true"></i>
                <span>My projects</span>
              </Button>
              <Button data-view="project-dashboard" className="nav">
                <i className="ti ti-chart-histogram" aria-hidden="true"></i>
                <span>Project dashboard</span>
              </Button>
            </section>
          </nav>
          <div className="sidebar-actions">
            <Button id="rotate-token" className="danger" data-break-glass-only>
              <i className="ti ti-refresh" aria-hidden="true"></i>
              <span>Rotate token</span>
            </Button>
            <Button id="sign-out">
              <i className="ti ti-logout" aria-hidden="true"></i>
              <span>Sign out</span>
            </Button>
            <div className="session-context" aria-label="Current session">
              <span className="environment-dot" aria-hidden="true"></span>
              <span>
                <strong id="session-name">Local</strong>
                <small id="session-role">Portal session</small>
              </span>
            </div>
          </div>
        </aside>

        <section className="workspace">
          <header className="global-toolbar">
            <div className="global-start">
              <Button
                id="nav-toggle"
                className="icon-button"
                type="button"
                aria-label="Open navigation"
                aria-controls="sidebar"
                aria-expanded="false"
              >
                <i className="ti ti-menu-2" aria-hidden="true"></i>
              </Button>
              <span id="scope-context">Workspace</span>
              <label className="project-scope-control">
                <span className="sr-only">Project scope</span>
                <NativeSelect id="project-scope">
                  <option value="">All projects</option>
                </NativeSelect>
              </label>
              <div className="breadcrumbs" aria-label="Breadcrumb" hidden>
                <span id="breadcrumb-domain">Monitor</span>
                <i className="ti ti-chevron-right" aria-hidden="true"></i>
                <strong id="breadcrumb-title">Overview</strong>
              </div>
            </div>
            <Button
              id="command-trigger"
              className="command-trigger"
              type="button"
              aria-haspopup="dialog"
              aria-label="Jump to a page"
              aria-keyshortcuts="Control+k Meta+k"
            >
              <i className="ti ti-search" aria-hidden="true"></i>
              <span>Jump to a page…</span>
              <kbd>⌘K</kbd>
            </Button>
            <div className="global-end">
              <label
                id="workspace-control"
                className="workspace-control hidden"
              >
                <span className="sr-only">Workspace</span>
                <NativeSelect id="workspace-select">
                  <option value="admin">Admin</option>
                  <option value="owner">Owner</option>
                </NativeSelect>
              </label>

              <span id="last-refreshed" className="last-refreshed">
                Not refreshed
              </span>
              <Button
                id="refresh"
                className="icon-button"
                type="button"
                aria-label="Refresh current view"
              >
                <i className="ti ti-refresh" aria-hidden="true"></i>
              </Button>
            </div>
          </header>
          <header className="view-toolbar">
            <div>
              <span id="view-domain" className="eyebrow">
                Monitor
              </span>
              <h2 id="view-title">Overview</h2>
              <p id="view-summary" className="view-summary">
                Gateway posture, traffic, and service availability.
              </p>
            </div>
            <div id="page-actions" className="actions"></div>
            <div className="governed-change" hidden>
              <Button
                id="governed-change-trigger"
                className="primary"
                type="button"
                aria-expanded="false"
                aria-controls="governed-change-menu"
              >
                <i className="ti ti-plus" aria-hidden="true"></i>
                <span>New governed change</span>
                <i className="ti ti-chevron-down" aria-hidden="true"></i>
              </Button>
              <div
                id="governed-change-menu"
                className="action-menu hidden"
                role="menu"
              >
                <Button type="button" role="menuitem" data-governed-view="keys">
                  <i className="ti ti-key" aria-hidden="true"></i>
                  <span>
                    <strong>Create key</strong>
                    <small>Issue a virtual key with policy</small>
                  </span>
                </Button>
                <Button
                  type="button"
                  role="menuitem"
                  data-governed-view="services"
                >
                  <i className="ti ti-cube-plus" aria-hidden="true"></i>
                  <span>
                    <strong>Register service</strong>
                    <small>Add an upstream service</small>
                  </span>
                </Button>
                <Button
                  type="button"
                  role="menuitem"
                  data-governed-view="providers"
                >
                  <i className="ti ti-database-plus" aria-hidden="true"></i>
                  <span>
                    <strong>Add provider</strong>
                    <small>Connect a model provider</small>
                  </span>
                </Button>
              </div>
            </div>
          </header>
          <section id="content" tabIndex={-1}></section>
        </section>
      </main>
    </>
  );
}
