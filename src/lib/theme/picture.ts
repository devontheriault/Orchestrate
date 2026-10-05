/**
 * The Picture theme's picture: asked of the user, and kept between launches.
 *
 * Kept in IndexedDB rather than beside the theme in localStorage, because a
 * photo is megabytes and localStorage is a few of them for the whole app. It
 * is kept as the file the user picked, not a path to it, so moving or
 * deleting the original doesn't blank the window.
 *
 * Chosen with a file input rather than the dialog plugin: the input hands
 * back the file's bytes, where the plugin hands back a path the page has no
 * permission to read.
 */

const DB = "orchestrate";
const STORE = "picture";
const KEY = "background";

function db(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function run<T>(mode: IDBTransactionMode, op: (s: IDBObjectStore) => IDBRequest<T>) {
  const conn = await db();
  try {
    return await new Promise<T>((resolve, reject) => {
      const req = op(conn.transaction(STORE, mode).objectStore(STORE));
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error);
    });
  } finally {
    conn.close();
  }
}

/** The stored picture, or null if none has been picked. */
export async function loadPicture(): Promise<File | null> {
  const file = await run<unknown>("readonly", (s) => s.get(KEY));
  return file instanceof Blob ? (file as File) : null;
}

export async function savePicture(file: File): Promise<void> {
  await run("readwrite", (s) => s.put(file, KEY));
}

/**
 * Open the OS's file chooser on images. Resolves to the file picked, or null
 * if the chooser is cancelled. Has to be called from inside a click or key
 * handler: a page may only open a chooser in answer to the user.
 */
export function choosePicture(): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/*";
    input.hidden = true;
    // WebKit only opens the chooser for an input that is in the document.
    document.body.append(input);
    const done = (file: File | null) => {
      input.remove();
      resolve(file);
    };
    input.addEventListener("change", () => done(input.files?.[0] ?? null), { once: true });
    input.addEventListener("cancel", () => done(null), { once: true });
    input.click();
  });
}
