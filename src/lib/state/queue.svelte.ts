/**
 * Messages the user has lined up behind a working Agent — see Queue in
 * CONTEXT.md. The Host keeps them on the Agent's record and sends the next one
 * when a Turn Completes, whether or not any window is open (ADR 0010). This is
 * the window's view of them and its handle on them.
 */

import { api, type Agent, type QueuedMessage } from "$lib/api";
import { DEFAULT_MODE } from "$lib/picks";
import { readJson } from "$lib/storage";
import type { AppStore } from "./store.svelte";

/**
 * Where this window kept queues before the Host did. Read once, handed to the
 * Host, and removed.
 */
const LEGACY_QUEUE_KEY = "cw:queues";

/** A message as the window stored it: "" meant no pick, and `mode` was absent before modes. */
type LegacyMessage = {
  id: string;
  prompt: string;
  attachments?: string[];
  model: string;
  effort: string;
  mode?: string;
};

function fromLegacy(m: LegacyMessage): QueuedMessage {
  return {
    id: m.id,
    prompt: m.prompt,
    attachments: m.attachments ?? [],
    model: m.model || null,
    effort: m.effort || null,
    // Queued before modes existed: it runs as every Turn ran back then.
    permission_mode: m.mode ?? DEFAULT_MODE,
  };
}

export class Queue {
  #app: AppStore;

  /** So a reconnect mid-handover can't hand the same messages over twice. */
  #handingOver = false;

  constructor(app: AppStore) {
    this.#app = app;
  }

  /** The queue behind one Agent, oldest first. */
  for(id: string | null): QueuedMessage[] {
    if (!id) return [];
    return this.#app.agents.find((a) => a.id === id)?.queue ?? [];
  }

  /** Drop one queued message — the ✕ beside it in the strip. */
  remove(agentId: string, messageId: string) {
    return this.#change(api.removeQueued(agentId, messageId));
  }

  /** Drop everything waiting behind an Agent. */
  clear(agentId: string) {
    return this.#change(api.clearQueue(agentId));
  }

  /**
   * Send the next message now. The Host only sends one by itself after a Turn
   * Completes, so a Stopped or Failed Agent's queue waits for this.
   */
  sendNext(agentId: string) {
    return this.#change(api.sendNext(agentId));
  }

  /**
   * Give the Host any queues this window kept before the Host kept them, so
   * nothing typed is lost in the move. Not sent: the Host holds them for the
   * user to send, since the Turns they were waiting on may be long over. What
   * the Host won't take stays here for the next try.
   */
  async handOver() {
    const stored = readJson<Record<string, LegacyMessage[]>>(LEGACY_QUEUE_KEY, {});
    if (this.#handingOver || Object.keys(stored).length === 0) return;
    this.#handingOver = true;
    for (const id of Object.keys(stored)) {
      const messages = stored[id] ?? [];
      // An Agent Discarded since: nothing left to hand its messages to.
      if (messages.length === 0 || !this.#app.agents.some((a) => a.id === id)) {
        delete stored[id];
        continue;
      }
      try {
        await api.queueMessages(id, messages.map(fromLegacy));
        delete stored[id];
      } catch {
        // Kept for the next launch.
      }
    }
    try {
      if (Object.keys(stored).length === 0) localStorage.removeItem(LEGACY_QUEUE_KEY);
      else localStorage.setItem(LEGACY_QUEUE_KEY, JSON.stringify(stored));
    } catch {
      // Worst case they are offered again next time; the Host keeps its copy.
    }
    this.#handingOver = false;
  }

  /**
   * Wait out a change. Its reply isn't put in place: the Host announces the
   * change too, in order with the Agent's other moves (see `AppStore.edit`).
   */
  async #change(call: Promise<Agent>) {
    try {
      await call;
    } catch (e) {
      this.#app.error = String(e);
    }
  }
}
