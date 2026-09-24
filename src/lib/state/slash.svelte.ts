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

  /**
   * What's known for `dir` on `host` — empty until the first ask comes back.
   * Keyed by both: a project is often at the same path on every machine.
   */
  for(host: string, dir: string | null | undefined): Entry {
    return (dir && this.entries[`${host}:${dir}`]) || EMPTY;
  }

  /**
   * Ask `claude` afresh. Called each time the menu opens rather than once:
   * skills get added mid-session, and the ask costs no tokens. The last list
   * stays up while it runs, so the menu never blanks on a reopen.
   */
  async load(host: string, dir: string) {
    const key = `${host}:${dir}`;
    const entry = this.for(host, dir);
    if (entry.loading) return;
    this.entries[key] = { ...entry, loading: true };
    try {
      const offered = await api.slashCommands(host, dir);
      this.entries[key] = {
        list: offered.commands,
        styles: offered.output_styles,
        loading: false,
        error: null,
      };
    } catch (e) {
      this.entries[key] = { ...this.for(host, dir), loading: false, error: String(e) };
    }
  }
}

export const slash = new SlashCommands();
