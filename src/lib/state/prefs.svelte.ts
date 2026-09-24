/**
 * The Model, Effort and Mode the user picked last, and the agent options they
 * set last, remembered so a new agent's page opens on their habitual choice
 * instead of resetting every time.
 */

import type { Agent, AgentOptions } from "$lib/api";
import { DEFAULT_EFFORT, DEFAULT_MODE, DEFAULT_MODEL } from "$lib/picks";
import type { AppStore } from "./store.svelte";

/**
 * The model the user picked last, for any Turn — a Spawn or a Resume — so a new
 * agent's page opens on their habitual choice instead of resetting every time.
 * localStorage, not sessionStorage: a preference should outlive the window.
 *
 * The stored value is the picker's own, so the empty string is a real answer —
 * the user asking for Claude Code's default. Only a missing key means they have
 * never picked, which is the one case the fallback below gets to speak.
 */
const MODEL_KEY = "cw:preferred-model";

/** The effort the user picked last, stored on the same terms as the model. */
const EFFORT_KEY = "cw:preferred-effort";

/** The mode the user picked last, stored on the same terms as the model. */
const MODE_KEY = "cw:preferred-mode";

/** The agent options the user set last, on any agent, as JSON. */
const OPTIONS_KEY = "cw:preferred-options";

function readStored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function readOptions(): AgentOptions {
  try {
    const parsed = JSON.parse(readStored(OPTIONS_KEY) ?? "{}");
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
}

export class TurnPrefs {
  #app: AppStore;

  /** The model the user picked for the most recent Turn. Null = never picked. */
  model = $state<string | null>(readStored(MODEL_KEY));

  /** The effort the user picked for the most recent Turn. Null = never picked. */
  effort = $state<string | null>(readStored(EFFORT_KEY));

  /** The mode the user picked for the most recent Turn. Null = never picked. */
  mode = $state<string | null>(readStored(MODE_KEY));

  /**
   * The options a new agent spawns with: the last the user set, on this page
   * or on any agent. Options are set rarely and meant to stick, so a user who
   * likes Concise replies shouldn't have to say so for every agent.
   */
  options = $state<AgentOptions>(readOptions());

  constructor(app: AppStore) {
    this.#app = app;
  }

  /**
   * The most recently spawned Agent, for the picks it ran on. Stands
   * in for a stored preference on a profile that has none yet — the Agents on
   * disk are a record of the user's picks too, and reading them means the picker
   * is right on the first spawn after an update rather than the second.
   */
  private lastSpawned = $derived.by(() => {
    let latest: Agent | null = null;
    for (const a of this.#app.agents) {
      if (!latest || Date.parse(a.spawned_at) >= Date.parse(latest.spawned_at)) {
        latest = a;
      }
    }
    return latest;
  });

  /** What a new agent's picker opens on: the last model the user ran on. */
  spawnModel = $derived(this.model ?? this.lastSpawned?.model ?? DEFAULT_MODEL);

  /** The same, for the effort level beside it. */
  spawnEffort = $derived(this.effort ?? this.lastSpawned?.effort ?? DEFAULT_EFFORT);

  /** And for the mode: what the last Turn ran as, else the YOLO default. */
  spawnMode = $derived(this.mode ?? this.lastSpawned?.permission_mode ?? DEFAULT_MODE);

  /**
   * Remember a model pick as the default for the next Spawn. Called for every
   * Turn the user starts, so "the model I ran last" is what the next one opens
   * on, whether that Turn was a Spawn or a reply to a running conversation.
   */
  remember(model: string, effort: string, mode: string) {
    this.model = model;
    this.effort = effort;
    this.mode = mode;
    try {
      localStorage.setItem(MODEL_KEY, model);
      localStorage.setItem(EFFORT_KEY, effort);
      localStorage.setItem(MODE_KEY, mode);
    } catch {
      // A preference isn't worth failing a turn over.
    }
  }

  /** Remember options just set as the ones the next Spawn opens on. */
  rememberOptions(options: AgentOptions) {
    this.options = options;
    try {
      localStorage.setItem(OPTIONS_KEY, JSON.stringify(options));
    } catch {
      // As above.
    }
  }
}
