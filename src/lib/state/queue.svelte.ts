/**
 * Messages the user has lined up behind a working Agent — see Queue in
 * CONTEXT.md. Only one `claude` may run in a Worktree at a time, so a queued
 * message is held here and sent as its own Turn when the Agent is free, rather
 * than racing the Turn already in flight.
 */

import { DEFAULT_MODE } from "$lib/picks";
import type { AppStore } from "./store.svelte";

/**
 * Messages the user has lined up behind a working Agent, by Agent id. Written
 * to localStorage on every change so a window reload — or a relaunch after the
 * app was closed mid-Turn — doesn't quietly throw away text the user typed.
 */
const QUEUE_KEY = "cw:queues";

/**
 * One prompt waiting for an Agent to be free, with the Model, effort and mode
 * the user picked for it. The pick travels with the message rather than being
 * read at send time: it's part of what the user decided when they queued it.
 */
export type QueuedMessage = {
  /** Local id, so the UI can delete one message out of the middle. */
  id: string;
  prompt: string;
  /** Files attached to it, by path. Absent on messages queued before attachments. */
  attachments?: string[];
  model: string;
  effort: string;
  /** Absent on messages queued before modes existed — those run the default. */
  mode?: string;
};

/**
 * A local handle for one queued message — it only has to be unique among the
 * messages this window is holding. A counter beside the clock rather than
 * `crypto.randomUUID`, which isn't guaranteed on every webview this app runs in.
 */
let queueSeq = 0;
function queuedId(): string {
  return `q${Date.now().toString(36)}-${queueSeq++}`;
}

function readQueues(): Record<string, QueuedMessage[]> {
  try {
    const raw = localStorage.getItem(QUEUE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, QueuedMessage[]>;
    // Anything malformed is worth less than a working queue: drop it.
    if (!parsed || typeof parsed !== "object") return {};
    return parsed;
  } catch {
    return {};
  }
}

export class Queue {
  #app: AppStore;

  /** Messages waiting behind each Agent, oldest first, by Agent id. */
  byAgent = $state<Record<string, QueuedMessage[]>>(readQueues());

  constructor(app: AppStore) {
    this.#app = app;
  }

  /** The queue behind one Agent, oldest first. */
  for(id: string | null): QueuedMessage[] {
    return id ? this.byAgent[id] ?? [] : [];
  }

  /**
   * Hold a prompt for the selected Agent until its current Turn ends. Returns
   * whether it was taken, on the same terms as a send: false leaves the text in
   * the composer rather than losing it.
   */
  enqueue(
    prompt: string,
    attachments: string[],
    model: string,
    effort: string,
    mode: string,
  ): boolean {
    const id = this.#app.selectedAgentId;
    const agent = this.#app.selectedAgent;
    if (!id || !agent?.session_id || !prompt.trim()) return false;
    this.byAgent[id] = [
      ...this.for(id),
      { id: queuedId(), prompt: prompt.trim(), attachments, model, effort, mode },
    ];
    this.save();
    // The Turn may have ended between the user typing and pressing Enter; in
    // that case the message shouldn't sit there waiting for a Turn that is
    // already over. drain only fires on an Agent that is free.
    if (agent.state !== "running") this.drain(id);
    return true;
  }

  /** Drop one queued message — the ✕ beside it in the strip. */
  remove(agentId: string, messageId: string) {
    const left = this.for(agentId).filter((m) => m.id !== messageId);
    if (left.length === 0) delete this.byAgent[agentId];
    else this.byAgent[agentId] = left;
    this.save();
  }

  /** Drop everything waiting behind an Agent. */
  clear(agentId: string) {
    if (!this.byAgent[agentId]) return;
    delete this.byAgent[agentId];
    this.save();
  }

  /**
   * Send the next queued message to an Agent that is free, and take it off the
   * queue once it's away. Left on the queue if the send fails, so a rejected
   * follow-up stays visible beside the error rather than vanishing.
   *
   * Only called for an Agent that ended a Turn *cleanly*, or when the user asks
   * for it by hand. A Stop or a Fail wants a human look — firing the rest of the
   * queue into a Stopped Agent would undo the interrupt the user just made.
   */
  async drain(agentId: string) {
    const [next] = this.for(agentId);
    if (!next) return;
    const agent = this.#app.agents.find((a) => a.id === agentId);
    if (!agent || agent.state === "running" || !agent.session_id) return;
    const sent = await this.#app.sendTurn(
      agentId,
      next.prompt,
      next.attachments ?? [],
      next.model,
      next.effort,
      // Queued before modes existed: run it as every Turn ran back then.
      next.mode ?? DEFAULT_MODE,
    );
    if (sent) {
      this.remove(agentId, next.id);
    }
  }

  /**
   * Forget queues belonging to Agents that no longer exist — Reaped in another
   * window, or gone since the last launch. A stored queue outliving its Agent
   * would otherwise never be sent and never be seen.
   */
  prune() {
    let dropped = false;
    for (const id of Object.keys(this.byAgent)) {
      if (!this.#app.agents.some((a) => a.id === id)) {
        delete this.byAgent[id];
        dropped = true;
      }
    }
    if (dropped) this.save();
  }

  private save() {
    try {
      localStorage.setItem(QUEUE_KEY, JSON.stringify(this.byAgent));
    } catch {
      // The queue still works in memory; persistence isn't worth an error over.
    }
  }
}
