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
