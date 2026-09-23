/**
 * The models this user's account can run, as Anthropic's Models API reports
 * them: what the model picker offers, and how a model id is named anywhere in
 * the UI.
 */

import { api, type ModelInfo } from "$lib/api";
import { modelLabel } from "$lib/picks";

class Models {
  /** The models this account can run, newest first. Empty until loaded. */
  list = $state<ModelInfo[]>([]);
  loading = $state<boolean>(false);
  /**
   * Why the model list couldn't be loaded, if it couldn't. Kept out of the
   * global error banner: the pickers stay usable on their Default option, so
   * this is a note beside them rather than something to interrupt over.
   */
  error = $state<string | null>(null);

  /** Ask the backend which models this account can run. */
  async load() {
    this.loading = true;
    this.error = null;
    try {
      this.list = await api.listModels();
    } catch (e) {
      this.list = [];
      this.error = String(e);
    } finally {
      this.loading = false;
    }
  }

  /**
   * Anthropic's name for a model id. Falls back to the id itself, which is what
   * an agent picked before the model left the account shows — better than
   * pretending it ran on something else.
   */
  name(id: string | null | undefined): string {
    if (!id) return "Default";
    const found = this.list.find((m) => m.id === id);
    return found ? modelLabel(found.display_name) : id;
  }
}

export const models = new Models();
