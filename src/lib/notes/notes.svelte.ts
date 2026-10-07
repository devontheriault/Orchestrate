/**
 * The Notes Space's state (ADR 0017): the notes on one Host, the one open in
 * the editor, and the search. The files are the store, so this is only the
 * window's reading of them, kept current by the Host's `notes-changed`.
 *
 * The open note autosaves. Every save names the version it was read at, so a
 * note that changed on disk meanwhile — in another editor, another window, or
 * by an Agent — is never overwritten: the Host answers with a conflict, and the
 * user picks which to keep.
 */

import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  api,
  events,
  LOCAL,
  type Note,
  type NoteHit,
  type NoteSummary,
} from "$lib/api";
import { hosts } from "$lib/state/hosts.svelte";
import { folderOf, isUntitled, namedPath } from "./notes";

/** How long typing pauses before the note is saved. */
const SAVE_AFTER = 600;

/** How long typing in the search box pauses before the Host is asked. */
const SEARCH_AFTER = 140;

/** Which Host keeps the notes, when the user has picked one. */
const HOST_KEY = "orchestrate:notes-host";

/** The note in the editor. */
export type OpenNote = {
  path: string;
  /** What the editor holds. */
  text: string;
  /** What the file held when last read or written, and its version. */
  saved: string;
  version: string;
  modified: number;
  /** The line to bring into view once it's shown, from a search hit. */
  line: number | null;
  /**
   * Set when the file changed on disk under unsaved edits, or a save found it
   * had: the file as it is now, or null if it is gone. Saving waits until the
   * user decides.
   */
  conflict: { current: Note | null } | null;
};

export type SaveState = "saved" | "editing" | "saving" | "failed";

class NotesStore {
  /** The Host the notes are on. */
  host = $state(LOCAL);
  /** Where the folder is on that Host. */
  folder = $state("");
  notes = $state<NoteSummary[]>([]);
  folders = $state<string[]>([]);
  /** The Host has answered at least once. */
  loaded = $state(false);
  /** Why the notes couldn't be listed, if they couldn't. */
  problem = $state<string | null>(null);
  /** The last thing that failed, for the Space to show until dismissed. */
  error = $state<string | null>(null);

  /** The folder the list is narrowed to; "" for every note. */
  within = $state("");

  query = $state("");
  /** The search's answer, or null while there's no query. */
  hits = $state<NoteHit[] | null>(null);

  open = $state<OpenNote | null>(null);
  saveState = $state<SaveState>("saved");

  private unlisten: UnlistenFn[] = [];
  private started = false;
  private saveTimer: ReturnType<typeof setTimeout> | undefined;
  private searchTimer: ReturnType<typeof setTimeout> | undefined;
  private refreshTimer: ReturnType<typeof setTimeout> | undefined;
  /** The save in flight, so another waits for it rather than racing it. */
  private saving: Promise<void> | null = null;
  /** Bumped by every search, so a slow answer to an old query is dropped. */
  private searchSeq = 0;

  /** The notes the list shows: those in `within`. */
  shown = $derived(
    this.within === ""
      ? this.notes
      : this.notes.filter((n) => n.path.startsWith(this.within + "/")),
  );

  async start() {
    if (this.started) return;
    this.started = true;
    try {
      const stored = localStorage.getItem(HOST_KEY);
      this.host = stored ?? hosts.own ?? hosts.home;
    } catch {
      this.host = hosts.own ?? hosts.home;
    }
    this.unlisten.push(
      await events.onNotesChanged(({ host, paths }) => {
        if (host !== this.host) return;
        this.changed(paths);
      }),
      // Back from a time out of touch, anything may have changed.
      hosts.onConnect((host) => {
        if (host === this.host) this.changed([]);
      }),
    );
    await this.refresh();
  }

  async stop() {
    await this.flush();
    for (const u of this.unlisten) u();
    this.unlisten = [];
    this.started = false;
  }

  /** Keep notes from `host` from now on. */
  async useHost(host: string) {
    if (host === this.host) return;
    await this.flush();
    this.host = host;
    try {
      localStorage.setItem(HOST_KEY, host);
    } catch {
      // Remembered for this window only, then.
    }
    this.open = null;
    this.within = "";
    this.notes = [];
    this.folders = [];
    this.loaded = false;
    await this.refresh();
  }

  async refresh() {
    try {
      const list = await api.notesList(this.host);
      this.folder = list.folder;
      this.notes = list.notes;
      this.folders = list.folders;
      this.problem = null;
      if (this.within && !list.folders.includes(this.within)) this.within = "";
    } catch (e) {
      this.problem = String(e);
    } finally {
      this.loaded = true;
    }
  }

  /** Something in the folder changed on disk. */
  private changed(paths: string[]) {
    clearTimeout(this.refreshTimer);
    this.refreshTimer = setTimeout(() => this.refresh(), 60);
    if (this.query.trim()) this.search(this.query);
    const open = this.open;
    if (open && (paths.length === 0 || paths.includes(open.path))) this.recheck(open.path);
  }

  /**
   * Read the open note again after it changed on disk. Unedited, the editor
   * simply shows the new text; with edits of its own, the user is asked.
   */
  private async recheck(path: string) {
    // A save of ours is what changed it, most likely: look once it's done.
    if (this.saving) await this.saving;
    const open = this.open;
    if (!open || open.path !== path) return;
    let current: Note | null;
    try {
      current = await api.notesRead(this.host, path);
    } catch {
      current = null;
    }
    if (this.open !== open) return;
    if (current?.version === open.version) return;
    if (open.text === open.saved && !open.conflict) {
      if (!current) {
        // Gone, with nothing of ours unsaved: nothing to keep open.
        this.open = null;
        return;
      }
      this.adopt(current);
      return;
    }
    if (current && current.content === open.text) {
      // The disk caught up with us — say, our own edit saved from elsewhere.
      this.adopt(current);
      return;
    }
    open.conflict = { current };
  }

  /** Show `note` as it is on disk, dropping whatever the editor held. */
  private adopt(note: Note) {
    this.open = {
      path: note.path,
      text: note.content,
      saved: note.content,
      version: note.version,
      modified: note.modified,
      line: null,
      conflict: null,
    };
    this.saveState = "saved";
  }

  /** Open a note, saving the one that was open first. */
  async openNote(path: string, line: number | null = null) {
    if (this.open?.path === path) {
      if (line) this.open.line = line;
      return;
    }
    await this.leave();
    try {
      const note = await api.notesRead(this.host, path);
      this.adopt(note);
      if (this.open) this.open.line = line;
    } catch (e) {
      this.error = String(e);
      this.refresh();
    }
  }

  /** Close the editor, saving first. */
  async close() {
    await this.leave();
    this.open = null;
  }

  /**
   * Save the open note if it needs it and, if it was made untitled and has a
   * title now, name its file after it — so the folder reads well in any
   * other app, not only this one.
   */
  private async leave() {
    await this.flush();
    const open = this.open;
    if (!open || open.conflict || !isUntitled(open.path)) return;
    const to = namedPath(open.path, open.text);
    if (!to || to === open.path) return;
    try {
      const moved = await api.notesRename(this.host, open.path, to);
      if (this.open === open) open.path = moved;
    } catch {
      // Taken, most likely: it stays Untitled, which is no harm.
    }
  }

  /** The editor's text changed. */
  edit(text: string) {
    const open = this.open;
    if (!open || open.text === text) return;
    open.text = text;
    this.saveState = "editing";
    clearTimeout(this.saveTimer);
    if (!open.conflict) this.saveTimer = setTimeout(() => this.save(), SAVE_AFTER);
  }

  /** Save now whatever is waiting to be. */
  async flush() {
    clearTimeout(this.saveTimer);
    await this.save();
  }

  private async save(): Promise<void> {
    if (this.saving) {
      await this.saving;
      return this.save();
    }
    const open = this.open;
    if (!open || open.conflict || open.text === open.saved) return;
    const text = open.text;
    this.saveState = "saving";
    this.saving = (async () => {
      try {
        const saved = await api.notesWrite(this.host, open.path, text, open.version);
        if (saved.outcome === "saved") {
          open.saved = text;
          open.version = saved.version;
          open.modified = saved.modified;
        } else {
          open.conflict = { current: saved.current };
        }
        if (this.open === open) {
          this.saveState = open.text === open.saved ? "saved" : "editing";
        }
      } catch (e) {
        if (this.open === open) this.saveState = "failed";
        this.error = `Couldn't save ${open.path}: ${e}`;
      }
    })();
    try {
      await this.saving;
    } finally {
      this.saving = null;
    }
    // Typed more while that was on its way.
    if (this.open === open && !open.conflict && open.text !== open.saved) {
      clearTimeout(this.saveTimer);
      this.saveTimer = setTimeout(() => this.save(), SAVE_AFTER);
    }
  }

  /**
   * Settle a conflict: keep the editor's text over the disk's, take the
   * disk's, or keep both by saving the editor's as a new note beside it.
   */
  async resolve(choice: "mine" | "theirs" | "both") {
    const open = this.open;
    if (!open?.conflict) return;
    const { current } = open.conflict;
    if (choice === "theirs") {
      if (current) this.adopt(current);
      else this.open = null;
      return;
    }
    if (choice === "both") {
      try {
        const copy = await api.notesCreate(
          this.host,
          folderOf(open.path) || null,
          `${open.path.slice(open.path.lastIndexOf("/") + 1).replace(/\.md$/i, "")} (mine)`,
          open.text,
        );
        if (current) this.adopt(current);
        await this.openNote(copy.path);
      } catch (e) {
        this.error = String(e);
      }
      return;
    }
    // Mine, knowingly: over the version now on disk, or as new if it's gone.
    open.conflict = null;
    open.version = current?.version ?? "";
    if (!current) {
      try {
        const saved = await api.notesWrite(this.host, open.path, open.text, null);
        if (saved.outcome === "saved") {
          open.saved = open.text;
          open.version = saved.version;
          this.saveState = "saved";
        } else {
          open.conflict = { current: saved.current };
        }
      } catch (e) {
        this.error = String(e);
      }
      return;
    }
    open.saved = current.content;
    await this.flush();
  }

  /** Start a note in the folder the list is narrowed to, and open it. */
  async create(): Promise<boolean> {
    try {
      const note = await api.notesCreate(this.host, this.within || null, null);
      await this.leave();
      this.adopt(note);
      this.clearSearch();
      this.notes = [
        { path: note.path, title: "Untitled", snippet: "", modified: note.modified },
        ...this.notes,
      ];
      return true;
    } catch (e) {
      this.error = String(e);
      return false;
    }
  }

  /** Rename or move the open note. `to` is relative to the notes folder. */
  async rename(to: string): Promise<boolean> {
    const open = this.open;
    if (!open) return false;
    await this.flush();
    try {
      open.path = await api.notesRename(this.host, open.path, to);
      await this.refresh();
      return true;
    } catch (e) {
      this.error = String(e);
      return false;
    }
  }

  /**
   * Delete the open note to the trash. Answers with why not, when there's no
   * trash to take it: the Space then asks before deleting it for good.
   */
  async trash(permanent = false): Promise<string | null> {
    const open = this.open;
    if (!open) return null;
    clearTimeout(this.saveTimer);
    try {
      const done = await api.notesDelete(this.host, open.path, permanent);
      if (done.outcome === "no_trash") return done.reason;
      if (this.open === open) this.open = null;
      this.notes = this.notes.filter((n) => n.path !== open.path);
      return null;
    } catch (e) {
      this.error = String(e);
      return null;
    }
  }

  /** Narrow the list to a folder, or with "" show every note. */
  narrow(folder: string) {
    this.within = folder;
  }

  search(query: string) {
    this.query = query;
    clearTimeout(this.searchTimer);
    if (!query.trim()) {
      this.hits = null;
      return;
    }
    const seq = ++this.searchSeq;
    this.searchTimer = setTimeout(async () => {
      try {
        const hits = await api.notesSearch(this.host, query);
        if (seq === this.searchSeq) this.hits = hits;
      } catch (e) {
        if (seq === this.searchSeq) this.error = String(e);
      }
    }, SEARCH_AFTER);
  }

  clearSearch() {
    this.searchSeq++;
    clearTimeout(this.searchTimer);
    this.query = "";
    this.hits = null;
  }

  /** Keep notes in `folder` on the Host from now on, or in `~/Notes` with null. */
  async setFolder(folder: string | null): Promise<boolean> {
    await this.close();
    try {
      const set = await api.setNotesFolder(this.host, folder);
      this.folder = set.folder;
      this.within = "";
      await this.refresh();
      return true;
    } catch (e) {
      this.error = String(e);
      return false;
    }
  }
}

export const notes = new NotesStore();
