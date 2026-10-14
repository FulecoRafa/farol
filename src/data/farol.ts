// The farol service (core/): the only thing the launcher talks to. It owns
// the search text, the results and the previews; this file types them.
// Needs the capability "service:farol".

export interface Action {
  id: string;
  title: string;
  /** "enter", "ctrl+c", ... */
  key: string;
}

export interface Item {
  id: string;
  title: string;
  subtitle?: string;
  /** "icon:name", "file:/path" (an image) or "ph:glyph" (Phosphor). */
  icon?: string;
  /** A character drawn instead of an icon (emoji). */
  glyph?: string;
  actions?: Action[];
  /** A file: previewable, draggable. */
  path?: string;
  /** Right-hand hint: an alias, a shortcut. */
  hint?: string;
}

export interface Section {
  provider: string;
  title: string;
  items: Item[];
}

export interface Results {
  seq: number;
  text: string;
  sections: Section[];
}

export type Preview =
  | { id: string; kind: "none" }
  | { id: string; kind: "text"; title: string; text: string; facts?: [string, string][] }
  | { id: string; kind: "image"; title: string; src: string; facts?: [string, string][] };

export interface Outcome {
  close?: boolean;
  query?: string;
  toast?: string;
}

const farol = () => use("farol");

/** Whether the launcher is open (hotkeys and the CLI flip it). */
export const open = () => farol().subscribe<{ open: boolean; nonce: number }>("ui")()?.open ?? false;
/** The launcher shows when open and no other view is asked for. */
export const launcherOpen = () => open() && view() === "";

const EMPTY: Results = { seq: 0, text: "", sections: [] };
export const results = () => farol().subscribe<Results>("results")() ?? EMPTY;

export const preview = () => farol().subscribe<Preview>("preview")();

export const search = (text: string) => farol().call("query", { text });
export const showPreview = (id: string) => farol().call("preview", { id });
export const hide = () => farol().call("hide");

export function activate(id: string, action: string, done?: (o: Outcome) => void) {
  farol().call<Outcome>("activate", { id, action }, (r) => {
    if (r.ok) done?.(r.value);
    else done?.({ toast: r.error });
  });
}

/** Which window the service asks for: "" (the launcher) or "settings". */
export const view = () => farol().subscribe<{ view: string }>("ui")()?.view ?? "";
export const openSettings = () => farol().call("settings");

// ── the settings window ────────────────────────────────────────────────

export interface Alias { name: string; target: string; title: string; subtitle: string; icon: string }
export interface Hotkey { keys: string; query: string; title: string; subtitle: string; icon: string; replaced: string }
export interface Config {
  aliases: Alias[];
  hotkeys: Hotkey[];
  desktop: { keys: string; title: string }[];
  links: { name: string; url: string }[];
  plugins: { name: string; title?: string; prefix?: string; mixed?: number; online: boolean }[];
  options: Record<string, unknown>;
  autostart: boolean;
}

const NO_CONFIG: Config = { aliases: [], hotkeys: [], desktop: [], links: [], plugins: [], options: {}, autostart: false };
export const config = () => farol().subscribe<Config>("config")() ?? NO_CONFIG;

/** An edit (`alias_add`, `option_set`...); `done` gets the error, if any. */
export function edit(method: string, args: Record<string, unknown>, done?: (error: string) => void) {
  farol().call(method, args, (r) => done?.(r.ok ? "" : r.error));
}

export function status(done: (s: Record<string, any>) => void) {
  farol().call<Record<string, any>>("status", null, (r) => r.ok && done(r.value));
}
