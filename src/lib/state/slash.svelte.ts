/**
 * The slash commands `claude` offers, per directory it was asked in: what the
 * composer's `/` menu lists. Per directory because a Project's own commands
 * and skills live in its tree, so two Projects offer different lists. The
 * output styles come in the same answer, for the same reason.
 */

import { api, type SlashCommand } from "$lib/api";

type Entry = {
  list: SlashCommand[];
  /** Output styles by name, for `/output-style` and the agent options. */
  styles: string[];
  loading: boolean;
  /** Why the last ask failed, if it did. Shown in the menu itself. */
  error: string | null;
};

const EMPTY: Entry = { list: [], styles: [], loading: false, error: null };

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
      const offered = await api.slashCommands(dir);
      this.entries[dir] = {
        list: offered.commands,
        styles: offered.output_styles,
        loading: false,
        error: null,
      };
    } catch (e) {
      this.entries[dir] = { ...this.for(dir), loading: false, error: String(e) };
    }
  }
}

export const slash = new SlashCommands();
