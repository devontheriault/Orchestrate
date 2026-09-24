/**
 * Which theme the app is wearing.
 *
 * The user picks System or one of the named themes, and the app resolves that
 * to the one value the stylesheet cares about, which it writes to
 * `<html data-theme>`. `src/lib/theme/theme.css` then has one block per theme —
 * Light and Dark there, the rest in `src/lib/theme/themes.css` — rather than a
 * media query plus an override of it.
 *
 * The same resolution runs as an inline script in `src/app.html`, before the
 * first paint, so the window never flashes the wrong theme on launch. The two
 * have to agree on the storage key and the attribute; the script is three
 * lines and deliberately duplicates that rather than waiting on this module.
 */

export type Mode = "light" | "dark";
export type ThemeId = (typeof THEMES)[number]["id"];
export type ThemePref = "system" | ThemeId;

/** Shared with the pre-paint script in `app.html`. */
export const STORAGE_KEY = "orchestrate:theme";

/**
 * The themes, in the order the menu lists them: the app's own first — Light,
 * Dark, and Orchestrate, the logo's palette, dark then light — then the named
 * palettes alphabetically, because after the app's own there is no ranking to
 * honour and alphabetical is the one order a reader can predict.
 *
 * `mode` is what the theme is, not what it resolves to — the menu groups by
 * it, and System picks the app's own Light or Dark. A theme's colours live in
 * `themes.css`; nothing but its name and its mode is needed here.
 */
export const THEMES = [
  { id: "light", name: "Light", mode: "light" },
  { id: "dark", name: "Dark", mode: "dark" },
  { id: "orchestrate", name: "Orchestrate", mode: "dark" },
  { id: "orchestrate-light", name: "Orchestrate Light", mode: "light" },
  { id: "catppuccin-latte", name: "Catppuccin Latte", mode: "light" },
  { id: "catppuccin-mocha", name: "Catppuccin Mocha", mode: "dark" },
  { id: "dracula", name: "Dracula", mode: "dark" },
  { id: "everforest", name: "Everforest", mode: "dark" },
  { id: "gruvbox-dark", name: "Gruvbox Dark", mode: "dark" },
  { id: "gruvbox-light", name: "Gruvbox Light", mode: "light" },
  { id: "monokai", name: "Monokai", mode: "dark" },
  { id: "nord", name: "Nord", mode: "dark" },
  { id: "one-dark", name: "One Dark", mode: "dark" },
  { id: "rose-pine", name: "Rosé Pine", mode: "dark" },
  { id: "rose-pine-dawn", name: "Rosé Pine Dawn", mode: "light" },
  { id: "solarized-dark", name: "Solarized Dark", mode: "dark" },
  { id: "solarized-light", name: "Solarized Light", mode: "light" },
  { id: "tokyo-night", name: "Tokyo Night", mode: "dark" },
] as const satisfies readonly { id: string; name: string; mode: Mode }[];

const BY_ID = new Map(THEMES.map((t) => [t.id as string, t]));

const DARK_QUERY = "(prefers-color-scheme: dark)";

function stored(): ThemePref {
  if (typeof localStorage === "undefined") return "system";
  const raw = localStorage.getItem(STORAGE_KEY);
  // Anything unrecognised — a theme that has since been removed, a value from
  // a future version — reads as System rather than as a broken window.
  return raw && BY_ID.has(raw) ? (raw as ThemeId) : "system";
}

function systemIsDark(): boolean {
  return typeof matchMedia !== "undefined" && matchMedia(DARK_QUERY).matches;
}

function resolve(pref: ThemePref, system: ThemeId): ThemeId {
  return pref === "system" ? system : pref;
}

class Theme {
  /** What the user chose. `system` means "whatever the OS is doing". */
  pref = $state<ThemePref>(stored());

  /**
   * A theme being tried on rather than chosen. The settings menu points this
   * at whichever row the pointer is over, so the window behind the menu shows
   * what picking that row would look like. Never stored, and it leaves `pref`
   * alone — dropping it back to `null` restores what the user actually chose.
   */
  preview = $state<ThemePref | null>(null);

  /** What the OS is doing, kept live so `system` follows it without a reload. */
  #systemDark = $state(systemIsDark());

  /**
   * What System resolves to right now — whatever the preference happens to
   * be, so the System row can say what picking it would get you rather than
   * describing the theme already on.
   */
  system = $derived<ThemeId>(this.#systemDark ? "dark" : "light");

  systemName = $derived(BY_ID.get(this.system)!.name);

  /** The value the stylesheet actually reads — a preview wins while one is up. */
  resolved = $derived<ThemeId>(resolve(this.preview ?? this.pref, this.system));

  /** Light or dark — what the resolved theme is, for anything outside CSS. */
  mode = $derived<Mode>(BY_ID.get(this.resolved)!.mode);

  /** Names the choice, not the preview: it labels the button that opens the menu. */
  label = $derived(
    this.pref === "system" ? `System (${this.systemName})` : BY_ID.get(this.pref)!.name,
  );

  /**
   * Mirror the resolved theme onto the document, and follow the OS while the
   * preference is `system`. Called once, from the root layout.
   *
   * `color-scheme` — the part of theming the stylesheet cannot reach, for
   * native controls and the canvas behind the page — rides along in each
   * theme's own block rather than being set here, so a new palette declares
   * which way it goes in the one place it is written.
   */
  start() {
    $effect(() => {
      document.documentElement.dataset.theme = this.resolved;
    });

    $effect(() => {
      const mq = matchMedia(DARK_QUERY);
      const onChange = (e: MediaQueryListEvent) => (this.#systemDark = e.matches);
      mq.addEventListener("change", onChange);
      return () => mq.removeEventListener("change", onChange);
    });
  }

  /**
   * Bumped to ask the settings menu to open on its themes — `/theme` typed
   * with no name. A count rather than a flag, so asking twice opens it twice.
   */
  pickerRequests = $state(0);

  openPicker() {
    this.pickerRequests++;
  }

  set(pref: ThemePref) {
    this.pref = pref;
    // The choice supersedes whatever was being tried on.
    this.preview = null;
    // `system` is the default, so it is stored as the absence of a choice —
    // a later change to what the default means then reaches existing users.
    if (pref === "system") localStorage.removeItem(STORAGE_KEY);
    else localStorage.setItem(STORAGE_KEY, pref);
  }
}

export const theme = new Theme();
