/**
 * Settings and caches the window keeps in localStorage. Every read and write
 * is best effort: storage can be full, switched off, or — while the page is
 * rendered ahead of time (ADR 0015) — not there at all.
 */

/**
 * The JSON stored under `key`, or `fallback` when there is none, it can't be
 * read, or it isn't the same kind of thing — an object for an object, an array
 * for an array.
 */
export function readJson<T extends object>(key: string, fallback: T): T {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(key) ?? "null");
    const fits =
      !!parsed && typeof parsed === "object" && Array.isArray(parsed) === Array.isArray(fallback);
    return fits ? (parsed as T) : fallback;
  } catch {
    return fallback;
  }
}

/** Store `value` as JSON under `key`, if storage will take it. */
export function writeJson(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // A setting that can't be kept still holds until the window closes.
  }
}
