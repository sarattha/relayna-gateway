import { createRoot } from "react-dom/client";
import { Button } from "../components/ui";

type Drawer = (
  title: string,
  form: HTMLElement,
  onClose: () => void,
  trigger: HTMLElement,
) => unknown;
export function mountRouteConfiguration(
  form: HTMLFormElement,
  openDrawer: Drawer,
) {
  const row = form.closest("tr");
  const route =
    row?.querySelector("code")?.textContent ||
    form.dataset.routeId ||
    form.dataset.serviceName ||
    "Route";
  const mode =
    form.querySelector<HTMLSelectElement>('[name="mode"]')?.selectedOptions[0]
      ?.textContent;
  const placeholder = document.createElement("div");
  placeholder.hidden = true;
  const host = document.createElement("div");
  host.dataset.reactUi = "";
  form.before(host, placeholder);
  placeholder.append(form);
  const root = createRoot(host);
  root.render(
    <div className={mode ? "grid grid-cols-[1fr_auto] items-center gap-3" : "flex items-center"}>
      {mode && (
        <span className="whitespace-nowrap text-xs text-muted-foreground">
          {mode.replaceAll("_", " ")}
        </span>
      )}
      <Button
        size="sm"
        aria-label={`Configure ${route}`}
        onClick={(event) =>
          openDrawer(
            `Configure ${route}`,
            form,
            () => placeholder.append(form),
            event.currentTarget,
          )
        }
      >
        Configure
      </Button>
    </div>,
  );
  const observer = new MutationObserver(() => {
    if (!host.isConnected) {
      observer.disconnect();
      root.unmount();
    }
  });
  observer.observe(document.body, { childList: true, subtree: true });
  return () => {
    observer.disconnect();
    root.unmount();
  };
}
