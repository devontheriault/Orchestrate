/**
 * The Mail Space's state: the account, its folders, the conversations in the
 * folder or search being shown, and the message open in the reading pane.
 * Everything comes from the Host that holds the account (`hosts.home`), which
 * reads it from the mail server; this is only the window's view of it.
 *
 * Archiving, deleting and marking read change the list at once and tell the
 * server after, putting things back if it refuses, so triage keeps up with
 * the keyboard.
 */

import {
  api,
  events,
  type MailFolder,
  type MailMessage,
  type MailSetup,
  type MailStatus,
  type MailThread,
} from "$lib/api";
import { hosts } from "$lib/state/hosts.svelte";
import { store } from "$lib/state/store.svelte";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { afterRemoval, latest, neighbour, taskFor, uidsByFolder } from "./list";

/** How long a burst of change events is gathered before the list is read. */
const SETTLE_MS = 400;

/** How soon a Host that didn't answer is asked again. */
const RETRY_MS = 2000;

class Mail {
  status = $state<MailStatus | null>(null);
  folders = $state<MailFolder[]>([]);
  /** The folder being shown, by path. */
  folder = $state<string>("INBOX");
  threads = $state<MailThread[]>([]);
  /** The search being shown instead of the folder, and what it found. */
  query = $state<string>("");
  results = $state<MailThread[] | null>(null);
  loading = $state(false);
  searching = $state(false);

  selectedId = $state<string | null>(null);
  /** The message open in the reading pane, and which one is loading. */
  open = $state<MailMessage | null>(null);
  opening = $state<string | null>(null);
  /** Messages whose remote images the user chose to load, by `folder:uid`. */
  imagesFor = $state<Record<string, boolean>>({});

  error = $state<string | null>(null);
  /** Why the Host couldn't be asked about mail, while it can't. */
  unreachable = $state<string | null>(null);
  /** A short word on what just happened: an Agent started, a file saved. */
  notice = $state<string | null>(null);

  /** The list shown: the search's, or the folder's. */
  list = $derived(this.results ?? this.threads);
  selected = $derived(this.list.find((t) => t.id === this.selectedId) ?? null);
  connected = $derived(this.status?.state === "connected");
  me = $derived(this.status?.account?.email ?? "");

  private unlisten: UnlistenFn | null = null;
  private users = 0;
  private settle: ReturnType<typeof setTimeout> | null = null;
  private noticeTimer: ReturnType<typeof setTimeout> | null = null;
  private retry: ReturnType<typeof setTimeout> | null = null;

  private get host() {
    return hosts.home;
  }

  /** Start showing mail. Counted, so a Space mounted twice starts once. */
  async start() {
    this.users++;
    if (this.users > 1) return;
    this.unlisten = await events.onMailChanged((change) => {
      if (change.host !== this.host) return;
      if (change.folder === null) void this.loadStatus();
      if (change.folder === null || change.folder === this.folder) this.soon();
    });
    await this.loadStatus();
  }

  stop() {
    this.users = Math.max(0, this.users - 1);
    if (this.users > 0) return;
    this.unlisten?.();
    this.unlisten = null;
  }

  /** Read the list again shortly, once a burst of changes has passed. */
  private soon() {
    if (this.settle) clearTimeout(this.settle);
    this.settle = setTimeout(() => {
      this.settle = null;
      void this.refresh(true);
    }, SETTLE_MS);
  }

  async loadStatus() {
    try {
      const was = this.status?.state;
      this.status = await api.mailStatus(this.host);
      this.unreachable = null;
      if (this.status.state === "connected" && was !== "connected") await this.refresh();
    } catch (e) {
      // The window may ask before its Host has answered at all, so keep
      // asking while the Space is open.
      this.unreachable = String(e);
      if (this.retry) clearTimeout(this.retry);
      this.retry = setTimeout(() => {
        this.retry = null;
        if (this.users > 0) void this.loadStatus();
      }, RETRY_MS);
    }
  }

  /** Read the folders and the shown folder's conversations afresh. */
  async refresh(quiet = false) {
    if (!this.status?.account) return;
    if (!quiet) this.loading = true;
    try {
      const [folders, threads] = await Promise.all([
        api.mailFolders(this.host),
        api.mailThreads(this.host, this.folder),
      ]);
      this.folders = folders;
      this.threads = threads;
      if (!this.selected && !quiet) this.selectedId = null;
      this.error = null;
    } catch (e) {
      if (!quiet) this.error = String(e);
    } finally {
      this.loading = false;
    }
  }

  async selectFolder(path: string) {
    if (path === this.folder && !this.results) return;
    this.folder = path;
    this.clearSearch();
    this.threads = [];
    this.selectedId = null;
    this.open = null;
    this.loading = true;
    try {
      this.threads = await api.mailThreads(this.host, path);
      this.error = null;
    } catch (e) {
      this.error = String(e);
    } finally {
      this.loading = false;
    }
  }

  async search(query: string) {
    this.query = query;
    if (!query.trim()) {
      this.clearSearch();
      return;
    }
    this.searching = true;
    try {
      this.results = await api.mailSearch(this.host, query, null);
      this.selectedId = null;
      this.open = null;
      this.error = null;
    } catch (e) {
      this.error = String(e);
    } finally {
      this.searching = false;
    }
  }

  clearSearch() {
    this.query = "";
    if (this.results) {
      this.results = null;
      this.selectedId = null;
      this.open = null;
    }
  }

  /** Show a conversation: its newest message opens, and the whole of it is read. */
  async select(id: string | null) {
    this.selectedId = id;
    const t = this.selected;
    if (!t) {
      this.open = null;
      return;
    }
    await this.openMessage(latest(t).folder, latest(t).uid);
    if (t.unread > 0) this.markRead(t, true);
  }

  async openMessage(folder: string, uid: number) {
    const key = `${folder}:${uid}`;
    this.opening = key;
    try {
      const m = await api.mailMessage(this.host, folder, uid, !!this.imagesFor[key]);
      if (this.opening === key) this.open = m;
    } catch (e) {
      this.error = String(e);
    } finally {
      if (this.opening === key) this.opening = null;
    }
  }

  /** Load the open message's remote images, which says it was read. */
  async loadImages() {
    if (!this.open) return;
    this.imagesFor[`${this.open.folder}:${this.open.uid}`] = true;
    await this.openMessage(this.open.folder, this.open.uid);
  }

  step(by: 1 | -1) {
    const next = neighbour(this.list, this.selectedId, by);
    if (next && next !== this.selectedId) void this.select(next);
  }

  /** Mark a conversation read or unread, here at once and then on the server. */
  markRead(t: MailThread, read: boolean) {
    const changed = t.messages.filter((m) => m.unread === read);
    if (changed.length === 0) return;
    this.setUnread(t.id, changed.map((m) => `${m.folder}:${m.uid}`), !read);
    for (const [folder, uids] of uidsByFolder({ ...t, messages: changed })) {
      api.mailSetSeen(this.host, folder, uids, read).catch((e) => {
        this.error = String(e);
        this.setUnread(t.id, changed.map((m) => `${m.folder}:${m.uid}`), read);
      });
    }
  }

  private setUnread(id: string, keys: string[], unread: boolean) {
    const counts = new Map<string, number>();
    for (const list of [this.threads, this.results ?? []]) {
      for (const t of list) {
        if (t.id !== id) continue;
        for (const m of t.messages) {
          if (!keys.includes(`${m.folder}:${m.uid}`) || m.unread === unread) continue;
          m.unread = unread;
          if (list === this.threads) counts.set(m.folder, (counts.get(m.folder) ?? 0) + (unread ? 1 : -1));
        }
        t.unread = t.messages.filter((m) => m.unread).length;
      }
    }
    for (const f of this.folders) {
      const d = counts.get(f.path);
      if (d) f.unread = Math.max(0, f.unread + d);
    }
  }

  toggleUnread() {
    const t = this.selected;
    if (t) this.markRead(t, t.unread > 0);
  }

  archive() {
    return this.remove("archive");
  }

  trash() {
    return this.remove("trash");
  }

  /** Take the selected conversation out of the list and move it on the server. */
  private async remove(how: "archive" | "trash") {
    const t = this.selected;
    if (!t) return;
    const before = { threads: this.threads, results: this.results };
    const next = afterRemoval(this.list, [t.id], t.id);
    this.threads = this.threads.filter((x) => x.id !== t.id);
    if (this.results) this.results = this.results.filter((x) => x.id !== t.id);
    void this.select(next);
    try {
      for (const [folder, uids] of uidsByFolder(t)) {
        await (how === "archive" ? api.mailArchive : api.mailTrash)(this.host, folder, uids);
      }
      this.say(how === "archive" ? "Archived" : "Moved to Trash");
      void this.refreshFolders();
    } catch (e) {
      this.error = String(e);
      this.threads = before.threads;
      this.results = before.results;
    }
  }

  private async refreshFolders() {
    try {
      this.folders = await api.mailFolders(this.host);
    } catch {
      // The counts are a nicety; the list is right either way.
    }
  }

  async setUp(setup: MailSetup): Promise<{ url?: string }> {
    const outcome = await api.mailSetUp(this.host, setup);
    if (outcome.next === "browser") {
      await this.loadStatus();
      return { url: outcome.url };
    }
    this.folder = "INBOX";
    await this.loadStatus();
    return {};
  }

  async signOut() {
    try {
      await api.mailSignOut(this.host);
    } catch (e) {
      this.error = String(e);
    }
    this.folders = [];
    this.threads = [];
    this.results = null;
    this.selectedId = null;
    this.open = null;
    await this.loadStatus();
  }

  /** Save an attachment of the open message to `path` on this machine. */
  async download(index: number, path: string) {
    if (!this.open) return;
    const d = await api.mailAttachment(this.host, this.open.folder, this.open.uid, index);
    const bytes = Uint8Array.from(atob(d.data), (c) => c.charCodeAt(0));
    await api.saveFile(path, bytes);
    this.say(`Saved ${d.name}`);
  }

  /** The selected conversation as the Task an Agent would be handed. */
  async taskFor(note: string): Promise<string> {
    const t = this.selected;
    if (!t) throw new Error("no conversation is selected");
    const parts: string[] = [];
    for (const [folder, uids] of uidsByFolder(t)) {
      parts.push(await api.mailThreadText(this.host, folder, uids));
    }
    return taskFor(note, parts.join("\n\n"));
  }

  /**
   * Spawn an Agent in `projectId` with the selected conversation as its Task,
   * through the same Spawn the Agents Space uses, on the picks the user ran
   * last. The new Agent is selected there, ready for when the user goes to it.
   */
  async sendToAgent(projectId: string, note: string): Promise<boolean> {
    const task = await this.taskFor(note);
    store.selectProject(projectId);
    const ok = await store.spawn(
      task,
      [],
      store.prefs.spawnModel,
      store.prefs.spawnEffort,
      store.prefs.spawnMode,
    );
    if (!ok) {
      this.error = store.error ?? "the agent didn't start";
      store.error = null;
      return false;
    }
    const name = store.projects.find((p) => p.id === projectId)?.name ?? "the project";
    this.say(`Agent started in ${name}`);
    return true;
  }

  say(text: string) {
    this.notice = text;
    if (this.noticeTimer) clearTimeout(this.noticeTimer);
    this.noticeTimer = setTimeout(() => (this.notice = null), 3500);
  }
}

export const mail = new Mail();
