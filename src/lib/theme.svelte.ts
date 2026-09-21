/**
 * Which theme the app is wearing.
 *
 * The user picks one of three things — Light, Dark, or System — and the app
 * resolves that to the one value the stylesheet cares about, which it writes
 * to `<html data-theme>`. `src/lib/theme.css` then has exactly one light block
 * and one dark block, rather than a media query plus an override of it.
 *
 * The same resolution runs as an inline script in `src/app.html`, before the
 * first paint, so the window never flashes the wrong theme on launch. The two
 * have to agree on the storage key and the attribute; the script is three
 * lines and deliberately duplicates that rather than waiting on this module.
 */

export type ThemePref = "system" | "light" | "dark";
export type Resolved = "light" | "dark";

/** Shared with the pre-paint script in `app.html`. */
export const STORAGE_KEY = "devcode:theme";

const ORDER: ThemePref[] = ["system", "light", "dark"];

const DARK_QUERY = "(prefers-color-scheme: dark)";

function stored(): ThemePref {
  if (typeof localStorage === "undefined") return "system";
  const raw = localStorage.getItem(STORAGE_KEY);
  return raw === "light" || raw === "dark" ? raw : "system";
}

function systemIsDark(): boolean {
  return typeof matchMedia !== "undefined" && matchMedia(DARK_QUERY).matches;
}

class Theme {
  /** What the user chose. `system` means "whatever the OS is doing". */
  pref = $state<ThemePref>(stored());

  /** What the OS is doing, kept live so `system` follows it without a reload. */
  #systemDark = $state(systemIsDark());

  /** The value the stylesheet actually reads. */
  resolved = $derived<Resolved>(
    this.pref === "system" ? (this.#systemDark ? "dark" : "light") : this.pref,
  );

  label = $derived(
    this.pref === "system" ? `System (${this.resolved})` : this.pref === "dark" ? "Dark" : "Light",
  );

  /**
   * Mirror the resolved theme onto the document, and follow the OS while the
   * preference is `system`. Called once, from the root layout.
   */
  start() {
    $effect(() => {
      document.documentElement.dataset.theme = this.resolved;
      // Tells the engine which way form controls, scrollbars and the canvas
      // behind the page should go — the parts the stylesheet cannot reach.
      document.documentElement.style.colorScheme = this.resolved;
    });

    $effect(() => {
      const mq = matchMedia(DARK_QUERY);
      const onChange = (e: MediaQueryListEvent) => (this.#systemDark = e.matches);
      mq.addEventListener("change", onChange);
      return () => mq.removeEventListener("change", onChange);
    });
  }

  set(pref: ThemePref) {
    this.pref = pref;
    // `system` is the default, so it is stored as the absence of a choice —
    // a later change to what the default means then reaches existing users.
    if (pref === "system") localStorage.removeItem(STORAGE_KEY);
    else localStorage.setItem(STORAGE_KEY, pref);
  }

  /** System → Light → Dark → System, for the single-button control. */
  cycle() {
    this.set(ORDER[(ORDER.indexOf(this.pref) + 1) % ORDER.length]);
  }
}

export const theme = new Theme();
