/**
 * The slash commands `claude` offers, per directory it was asked in: what the
 * composer's `/` menu lists. Per directory because a Project's own commands
 * and skills live in its tree, so two Projects offer different lists.
 */

import { api, type SlashCommand } from "$lib/api";

type Entry = {
  list: SlashCommand[];
  loading: boolean;
  /** Why the last ask failed, if it did. Shown in the menu itself. */
  error: string | null;
};

const EMPTY: Entry = { list: [], loading: false, error: null };

class SlashCommands {
  private entries = $state<Record<string, Entry>>({});

  /** What's known for `dir` — empty until the first ask comes back. */
  for(dir: string | null | undefined): Entry {
    return (dir && this.entries[dir]) || EMPTY;
  }

  /**
   * Ask `claude` afresh. Called each time the menu opens rather than once:
   * skills get added mid-session, and the ask costs no tokens. The last list
   * stays up while it runs, so the menu never blanks on a reopen.
   */
  async load(dir: string) {
    const entry = this.for(dir);
    if (entry.loading) return;
    this.entries[dir] = { ...entry, loading: true };
    try {
      this.entries[dir] = { list: await api.slashCommands(dir), loading: false, error: null };
    } catch (e) {
      this.entries[dir] = { ...this.for(dir), loading: false, error: String(e) };
    }
  }
}

export const slash = new SlashCommands();
