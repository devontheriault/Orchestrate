/**
 * The three choices a user makes for every Turn — the Model it runs on, the
 * Effort it thinks at, and the Permission Mode it runs in — as the pickers
 * offer them. The empty value of each means "don't pass the flag", leaving the
 * choice to Claude Code.
 */

/**
 * The picker's value for "no model named", which passes no `--model` and lets
 * Claude Code use whatever it is configured for. Always available, so the
 * picker still works when the models call fails.
 */
export const DEFAULT_MODEL = "";

/**
 * Anthropic's display names lead with "Claude", which is noise in an app that
 * runs nothing else: "Claude Opus 4.5" is just "Opus 4.5" here.
 */
export function modelLabel(displayName: string): string {
  return displayName.replace(/^claude\s+/i, "");
}

/**
 * How hard Claude Code works a turn, as `claude --effort` takes it. The picker
 * offers these under each model; the empty value passes no `--effort` and
 * leaves the level to Claude Code.
 */
export const EFFORTS = ["low", "medium", "high", "xhigh", "max"] as const;

/** The picker's value for "no effort named". */
export const DEFAULT_EFFORT = "";

/**
 * What a turn may do without asking, as `claude --permission-mode` takes it.
 * Only the two modes that never stop to ask are offered: the app hands `claude`
 * no input channel, so a mode that puts up a permission prompt would hang the
 * turn with nowhere to answer it. Isolation is the safety story here — the
 * worktree is the sandbox — so acting freely inside one is the default, and
 * plan is for the agent you want thinking before it touches anything. The
 * names here are the UI's; the ids are what `claude` takes.
 */
export const MODES = [
  { id: "bypassPermissions", name: "YOLO", note: "acts freely in its worktree" },
  { id: "plan", name: "Plan", note: "reads and proposes, writes nothing" },
] as const;

/** The mode a turn runs in unless the user picks otherwise. */
export const DEFAULT_MODE = "bypassPermissions";

/**
 * The modes as the picker lists them, with `value` among them. A mode this
 * build doesn't know still gets a row, so a record written by a newer version
 * reads as what it is rather than silently as YOLO.
 */
export function modeChoices(value: string): { id: string; name: string; note: string }[] {
  const known = MODES.some((m) => m.id === value);
  return [...MODES, ...(known ? [] : [{ id: value, name: value, note: "" }])];
}

/** What to call a mode. Falls back to the id, for a mode this build predates. */
export function modeLabel(id: string | null | undefined): string {
  if (!id) return modeLabel(DEFAULT_MODE);
  return MODES.find((m) => m.id === id)?.name ?? id;
}

/**
 * The advisors an agent can consult, as Claude Code's `advisorModel` setting
 * takes them. One of an agent's options rather than a per-Turn pick: set once
 * for the agent, and null leaves it to Claude Code's own configuration.
 */
export const ADVISORS = [
  { id: "off", name: "Off" },
  { id: "fable", name: "Fable" },
  { id: "opus", name: "Opus" },
  { id: "sonnet", name: "Sonnet" },
] as const;

/** What to call an advisor. Falls back to the id, for one this build predates. */
export function advisorLabel(id: string): string {
  return ADVISORS.find((a) => a.id === id)?.name ?? id;
}
