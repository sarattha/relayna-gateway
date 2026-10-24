export type ThemePreference = "light" | "dark" | "system";
export type ResolvedTheme = "light" | "dark";

export const THEME_STORAGE_KEY = "relayna.theme";
export const THEME_CHANGE_EVENT = "relayna:theme-change";

let preference: ThemePreference = "system";
let changePreference: ((value: ThemePreference) => void) | undefined;
let dispose: (() => void) | undefined;

function validPreference(value: unknown): value is ThemePreference {
  return value === "light" || value === "dark" || value === "system";
}

export function getThemePreference(): ThemePreference {
  return preference;
}

export function subscribeThemePreference(listener: () => void): () => void {
  document.addEventListener(THEME_CHANGE_EVENT, listener);
  return () => document.removeEventListener(THEME_CHANGE_EVENT, listener);
}

export function setThemePreference(value: ThemePreference): void {
  if (validPreference(value)) changePreference?.(value);
}

/** Initialize before React and operational controllers; return listener cleanup. */
export function initializeThemePreference(): () => void {
  dispose?.();
  let media: MediaQueryList | undefined;
  try {
    media = window.matchMedia("(prefers-color-scheme: dark)");
  } catch {
    // A browser without matchMedia keeps System usable with a light fallback.
  }
  let removeMediaListener: (() => void) | undefined;

  function apply(value: ThemePreference) {
    const previousPreference = preference;
    preference = value;
    const theme: ResolvedTheme =
      value === "system" ? (media?.matches ? "dark" : "light") : value;
    const previousTheme = document.documentElement.dataset.theme;
    document.documentElement.dataset.theme = theme;
    document.documentElement.dataset.rnTheme = theme;

    const query = media;
    if (value === "system" && !removeMediaListener && query) {
      if (typeof query.addEventListener === "function") {
        query.addEventListener("change", onSystemChange);
        removeMediaListener = () =>
          query.removeEventListener("change", onSystemChange);
      } else if (typeof query.addListener === "function") {
        query.addListener(onSystemChange);
        removeMediaListener = () => query.removeListener(onSystemChange);
      }
    } else if (value !== "system") {
      removeMediaListener?.();
      removeMediaListener = undefined;
    }

    // Preference-only changes synchronize both controls. Chart consumers can
    // compare detail.theme with their current theme to skip redundant work.
    if (previousTheme !== theme || previousPreference !== value) {
      document.dispatchEvent(
        new CustomEvent(THEME_CHANGE_EVENT, {
          detail: { theme, preference: value },
        }),
      );
    }
  }

  function onSystemChange() {
    if (preference === "system") apply("system");
  }

  function onStorage(event: StorageEvent) {
    if (event.key !== THEME_STORAGE_KEY && event.key !== null) return;
    try {
      if (event.storageArea && event.storageArea !== window.localStorage) return;
    } catch {
      // Blocked storage must not disable the in-memory preference.
    }
    apply(validPreference(event.newValue) ? event.newValue : "system");
  }

  const change = (value: ThemePreference) => {
    try {
      // Only this enum is persisted; authentication and form state stay outside.
      window.localStorage.setItem(THEME_STORAGE_KEY, value);
    } catch {
      // The current page still works when storage is blocked or full.
    }
    apply(value);
  };
  changePreference = change;
  let saved: string | null = null;
  try {
    saved = window.localStorage.getItem(THEME_STORAGE_KEY);
  } catch {
    // Default to System when the browser cannot read a preference.
  }
  apply(validPreference(saved) ? saved : "system");
  window.addEventListener("storage", onStorage);

  const cleanup = () => {
    window.removeEventListener("storage", onStorage);
    removeMediaListener?.();
    removeMediaListener = undefined;
    if (changePreference === change) {
      changePreference = undefined;
      dispose = undefined;
    }
  };
  dispose = cleanup;
  return cleanup;
}
