/**
 * The slash commands the app answers itself instead of handing them to `claude`.
 *
 * Every Turn here is its own `claude --print` process. A command that changes
 * the terminal (`/color` tints a prompt bar nobody sees) or a setting for the
 * session (`/model`, `/effort`) would reach a process that exits once it has
 * replied, and the next Turn would start without it. So the composer catches
 * those and does the app's version of each: `/model` moves the model picker,
 * `/color` tags the agent in the sidebar. The commands with no version here and
 * no use through the app are kept out of the menu, and turned back with the
 * reason if they are typed anyway.
 *
 * Pure: a prompt in, what to do about it out. The composer does it.
 */

import type { ModelInfo, SlashCommand } from "$lib/api";
import { ADVISORS, EFFORTS, modelLabel } from "$lib/picks";
import { TAGS, type Tag } from "$lib/theme/tags";
import { THEMES, type ThemePref } from "$lib/theme/theme.svelte";

/** A row in the `/` menu. `app` marks one the app answers rather than `claude`. */
export type MenuCommand = SlashCommand & { app?: boolean };

function ours(name: string, description: string, argument_hint = ""): MenuCommand {
  return { name, description, argument_hint, aliases: [], app: true };
}

/**
 * The app's commands, under the names Claude Code gives the same things so they
 * are where a Claude Code user's fingers already go — plus `/theme`, which
 * Claude Code has no need of.
 */
export const APP_COMMANDS: MenuCommand[] = [
  ours("advisor", "Let this agent consult a stronger model at key moments", "<off|fable|opus|sonnet|default>"),
  ours("color", "Tag this agent with a colour in the sidebar", `<${TAGS.join("|")}|default>`),
  ours("effort", "Set the effort this agent's next prompts run at", `<${EFFORTS.join("|")}|default>`),
  ours("model", "Set the model this agent's next prompts run on", "<model>"),
  ours("output-style", "Set how this agent writes its replies", "<style>"),
  ours("rename", "Rename this agent — with no name, Claude names it again", "[name]"),
  ours("theme", "Change the app's theme — with no name, pick from the list", "[theme]"),
  ours("usage", "Open the usage window: tokens, cost and plan limits"),
];

/**
 * Why each command the menu leaves out is left out, said if one is typed
 * anyway. They would run, but to no effect, or to one the user can't see or
 * stop — `/loop` claims to have scheduled a job in a process about to exit.
 */
const WITHHELD: Record<string, string> = {
  clear:
    "each agent here is one conversation, and clearing it would leave the transcript on screen over an empty session. Spawn a new agent (n) to start fresh.",
  fast: "fast mode isn't available to apps built on Claude Code.",
  loop: "it needs a session that stays open between runs, and each reply here is a fresh claude that exits when it's done.",
  batch: "it would spawn agents in worktrees this app can't see or stop. Spawn them here instead.",
  debug: "it would only log this one reply, since each reply is a fresh claude.",
  autocompact: "it would only last this one reply, since each reply is a fresh claude.",
  "reload-skills": "no need: each reply is a fresh claude, which loads skills anew.",
  "reload-plugins": "no need: each reply is a fresh claude, which loads plugins anew.",
  heapdump: "it would dump the memory of a claude that exits straight after.",
  "auto-mode-setup": "it's a wizard for scripts, and agents here run in YOLO or Plan only.",
  agents: "Claude Code has removed it. Subagents live in .claude/agents/.",
  "extra-usage": "Claude Code renamed it /usage-credits.",
  "workflow-launch-exec": "it's internal to Claude Code.",
};

/**
 * What the `/` menu offers: the app's commands, then what `claude` listed
 * less the ones the app answers or withholds.
 */
export function menuCommands(fromClaude: SlashCommand[]): MenuCommand[] {
  const answered = new Set(APP_COMMANDS.map((c) => c.name));
  return [
    ...APP_COMMANDS,
    ...fromClaude.filter((c) => !answered.has(c.name) && !(c.name in WITHHELD)),
  ];
}

/** What to do about a prompt the app answers. */
export type Action =
  | { do: "model"; model: string }
  | { do: "effort"; effort: string }
  | { do: "color"; color: Tag | null }
  | { do: "rename"; name: string | null }
  | { do: "advisor"; advisor: string | null }
  | { do: "style"; style: string | null }
  /** Null asks for the theme list rather than naming a theme. */
  | { do: "theme"; theme: ThemePref | null }
  | { do: "usage" }
  /** Not done, and why — a withheld command, or a choice there's no such thing as. */
  | { do: "refuse"; why: string };

/** What the answers are chosen from: this account's models, the directory's styles. */
export type Choices = { models: ModelInfo[]; styles: string[] };

/**
 * What to do about `prompt`, if it is a command the app answers or withholds.
 * Null for everything else, which goes to `claude` as typed.
 */
export function interpret(prompt: string, choices: Choices): Action | null {
  const m = /^\/(\S+)(?:\s+([\s\S]*))?$/.exec(prompt.trim());
  if (!m) return null;
  const name = m[1];
  const arg = (m[2] ?? "").trim();
  if (name in WITHHELD) return refuse(`/${name} isn't used here: ${WITHHELD[name]}`);

  const reset = ["default", "none", "auto"].includes(arg.toLowerCase());
  switch (name) {
    case "model": {
      if (reset) return { do: "model", model: "" };
      const model = findModel(arg, choices.models);
      if (model !== null) return { do: "model", model };
      const names = choices.models.map((x) => modelLabel(x.display_name));
      return refuse(
        arg ? `No model called “${arg}”. Try ${oneOf([...names, "default"])}.` : `Name a model: ${oneOf([...names, "default"])}.`,
      );
    }
    case "effort": {
      if (reset) return { do: "effort", effort: "" };
      const effort = EFFORTS.find((e) => e === arg.toLowerCase());
      return effort ? { do: "effort", effort } : refuse(`Effort is one of ${oneOf([...EFFORTS, "default"])}.`);
    }
    case "color": {
      if (reset) return { do: "color", color: null };
      const color = TAGS.find((t) => t === arg.toLowerCase());
      return color ? { do: "color", color } : refuse(`Pick a colour: ${oneOf([...TAGS, "default"])}.`);
    }
    case "advisor": {
      if (["default", "none"].includes(arg.toLowerCase())) return { do: "advisor", advisor: null };
      const advisor = ADVISORS.find((a) => a.id === arg.toLowerCase());
      return advisor
        ? { do: "advisor", advisor: advisor.id }
        : refuse(`The advisor is one of ${oneOf([...ADVISORS.map((a) => a.id), "default"])}.`);
    }
    case "output-style": {
      if (["default", "none"].includes(arg.toLowerCase())) return { do: "style", style: null };
      const styles = choices.styles.filter((s) => s !== "default");
      const style = styles.find((s) => norm(s) === norm(arg));
      if (style) return { do: "style", style };
      // Before the list has loaded, trust the name: `claude` knows its own styles.
      if (arg && !styles.length) return { do: "style", style: arg };
      return refuse(`The output style is one of ${oneOf([...styles, "default"])}.`);
    }
    case "rename":
      return { do: "rename", name: arg || null };
    case "theme": {
      if (!arg) return { do: "theme", theme: null };
      if (norm(arg) === "system") return { do: "theme", theme: "system" };
      const theme = THEMES.find((t) => norm(t.id) === norm(arg) || norm(t.name) === norm(arg));
      return theme
        ? { do: "theme", theme: theme.id }
        : refuse(`No theme called “${arg}”. Type /theme on its own to pick from the list.`);
    }
    case "usage":
      return { do: "usage" };
  }
  return null;
}

/**
 * The model `arg` names, as the picker holds it: an exact id or name, else the
 * newest model whose id contains it — so `opus` is the latest Opus, as it is to
 * `claude --model`. Null for no match. With no model list to look in, the name
 * goes through as typed and `claude` resolves it.
 */
function findModel(arg: string, models: ModelInfo[]): string | null {
  if (!arg) return null;
  if (!models.length) return arg;
  const key = norm(arg);
  const exact = models.find((m) => norm(m.id) === key || norm(modelLabel(m.display_name)) === key);
  return (exact ?? models.find((m) => norm(m.id).includes(key)))?.id ?? null;
}

/** For matching what's typed: case, spaces, dots and dashes don't count. */
function norm(s: string): string {
  return s.toLowerCase().replace(/[^a-z0-9]/g, "");
}

function oneOf(names: string[]): string {
  return names.length < 2 ? names.join("") : `${names.slice(0, -1).join(", ")} or ${names.at(-1)}`;
}

function refuse(why: string): Action {
  return { do: "refuse", why };
}
