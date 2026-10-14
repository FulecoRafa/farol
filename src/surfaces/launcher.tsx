// The launcher window: a field, the results grouped by provider on the left,
// the selected row's preview on the right, its actions along the bottom.
// The service answers every keystroke; this file only keeps the selection
// and draws the rows in view.
//
// Keys: ↑↓ (Ctrl+P/N, Ctrl+K/J) move, Tab jumps to the next group, Page
// Up/Down by a page, Enter runs the first action, the chords in the footer
// run the others, Escape clears the field and then closes.

import { Icon } from "@kit/components/Icon";
import * as farol from "../data/farol";
import type { Item } from "../data/farol";
import { ItemIcon } from "../components/ItemIcon";
import { PreviewPane } from "../components/PreviewPane";

const WIDTH = 860;
const HEIGHT = 520;
const LIST_WIDTH = 440;
const ROW = 44;
/** Rows in view; the list scrolls by moving this window. */
const PAGE = 8;

/** Prefix → the field's icon and a hint of the mode. */
const MODES: Record<string, [string, string]> = {
  "=": ["calculator", "calculadora"],
  ":": ["smiley", "emoji"],
  ">": ["clipboard-text", "área de transferência"],
  "/": ["file-magnifying-glass", "arquivos"],
  "!": ["skull", "encerrar janelas"],
  "?": ["globe", "web"],
};

interface Row {
  item: Item;
  section: string;
  /** First row of its section (draws the section title). */
  head: boolean;
  index: number;
}

export default function Launcher() {
  const rows = memo<Row[]>(() => {
    const out: Row[] = [];
    for (const s of farol.results().sections) {
      s.items.forEach((item, i) => out.push({ item, section: s.title, head: i === 0, index: out.length }));
    }
    return out;
  });

  // The selection belongs to one answer: a new `seq` starts at the top.
  const cursor = signal({ seq: 0, index: 0, top: 0 });
  const view = memo(() => {
    const c = cursor();
    const seq = farol.results().seq;
    return c.seq === seq ? c : { seq, index: 0, top: 0 };
  });
  // Narrow reads: a row re-checks only `index`, the list only `top`.
  const index = memo(() => view().index);
  const top = memo(() => view().top);
  const selected = memo<Item | null>(() => rows()[index()]?.item ?? null);

  const toast = signal("");
  let clearToast = () => {};
  const say = (text: string) => {
    clearToast();
    toast.set(text);
    clearToast = after(2500, () => toast.set(""));
  };

  const select = (index: number) => {
    const all = rows();
    if (all.length === 0) return;
    const i = Math.max(0, Math.min(all.length - 1, index));
    const v = view();
    const top = i < v.top ? i : i >= v.top + PAGE ? i - PAGE + 1 : v.top;
    cursor.set({ seq: v.seq, index: i, top });
    farol.showPreview(all[i].item.id);
  };

  const run = (item: Item | null, action: string) => {
    if (!item) return;
    farol.activate(item.id, action, (o) => {
      if (o.query !== undefined) farol.search(o.query);
      if (o.toast) say(o.toast);
    });
  };

  const nextSection = (dir: 1 | -1) => {
    const all = rows();
    let i = view().index;
    const here = all[i]?.section;
    do i += dir; while (i >= 0 && i < all.length && all[i].section === here);
    if (dir < 0) while (i > 0 && all[i - 1].section === all[i].section) i--;
    select(i < 0 ? all.length - 1 : i >= all.length ? 0 : i);
  };

  const onKey = (e: { key: string; value: string }) => {
    const k = e.key;
    if (k === "escape") return e.value ? farol.search("") : farol.hide();
    if (k === "down" || k === "ctrl+n" || k === "ctrl+j") return select(view().index + 1);
    if (k === "up" || k === "ctrl+p" || k === "ctrl+k") return select(view().index - 1);
    if (k === "pagedown") return select(view().index + PAGE);
    if (k === "pageup") return select(view().index - PAGE);
    if (k === "tab") return nextSection(1);
    if (k === "shift+tab") return nextSection(-1);
    const item = selected();
    const action = item?.actions?.find((a) => a.key === k);
    if (action) run(item, action.id);
  };

  const mode = () => MODES[farol.results().text.trimStart()[0] ?? ""];

  const field = () => (
    <Box height={64} padding={[0, 22]} gap={14} align="center">
      <Icon name={() => mode()?.[0] ?? "magnifying-glass"} size={20} color="$textMuted" />
      <Input
        value={() => farol.results().text}
        placeholder="buscar…   = conta · : emoji · / arquivos · > clipboard · ! janelas · ? web"
        size={17}
        autofocus
        onChange={(e) => farol.search(e.value)}
        onSubmit={() => {
          const item = selected();
          run(item, item?.actions?.[0]?.id ?? "");
        }}
        onKey={onKey}
      />
      {() => (mode() ? <Label text={mode()![1]} size={11} color="$textFaint" /> : null)}
    </Box>
  );

  const row = (r: Row) => (
    <Box key={r.item.id} direction="column">
      {r.head ? (
        <Box height={26} padding={[0, 14]} align="end">
          <Label text={r.section} size={10} weight="semibold" color="$textGhost" />
        </Box>
      ) : null}
      <Box
        height={ROW}
        padding={[0, 12]}
        gap={12}
        align="center"
        radius={12}
        bg={() => (index() === r.index ? "$raised" : "#00000000")}
        hoverBg="$hover"
        drag={r.item.path || undefined}
        onDragStart={r.item.path ? farol.hide : undefined}
        onClick={() => (index() === r.index ? run(r.item, r.item.actions?.[0]?.id ?? "") : select(r.index))}
      >
        <ItemIcon item={r.item} size={26} />
        <Box direction="column" gap={1} grow>
          <Label text={r.item.title} size={13} color="$textBright" maxWidth={300} />
          {r.item.subtitle ? <Label text={r.item.subtitle} size={11} color="$textFaint" maxWidth={300} /> : null}
        </Box>
        {r.item.hint ? <Label text={r.item.hint} size={11} color="$yellow" /> : null}
      </Box>
    </Box>
  );

  const list = () => {
    const all = rows();
    if (all.length === 0) {
      const empty = farol.results().text.trim() === "" ? "comece a digitar" : "nada encontrado";
      return (
        <Box width={LIST_WIDTH} padding={20}>
          <Label text={empty} size={12} color="$textFaint" />
        </Box>
      );
    }
    // A section title stays on top of the window even mid-section.
    const shown = all.slice(top(), top() + PAGE).map((r, i) => (i === 0 && !r.head ? { ...r, head: true } : r));
    return (
      <Box width={LIST_WIDTH} direction="column" gap={2} padding={[0, 8, 8, 8]}>
        {shown.map(row)}
      </Box>
    );
  };

  const footer = () => {
    const t = toast();
    const actions = selected()?.actions ?? [];
    return (
      <Box height={38} padding={[0, 22]} gap={18} align="center" bg="$sunken" radius={[0, 0, 24, 24]}>
        {t ? (
          <Label text={t} size={11} color="$green" />
        ) : (
          actions.map((a, i) => (
            <Box key={a.id} gap={6} align="center" onClick={() => run(selected(), a.id)}>
              <Label text={a.key === "enter" ? "↵" : a.key} size={11} color={i === 0 ? "$yellow" : "$textMuted"} />
              <Label text={a.title} size={11} color="$textFaint" />
            </Box>
          ))
        )}
      </Box>
    );
  };

  return (
    <Surface
      layer="overlay"
      anchor="top"
      width={WIDTH}
      height={HEIGHT}
      margin={[120, 0, 0, 0]}
      keyboard="exclusive"
      visible={farol.open}
      onDismiss={farol.hide}
    >
      <Box grow direction="column" radius={24} bg="$surface" border="$surfaceBorder">
        {field()}
        <Box height={1} bg="$divider" />
        <Box grow gap={8} padding={[4, 0, 0, 0]}>
          {list}
          <Box width={1} bg="$divider" />
          <Box grow padding={[12, 18, 12, 10]}>
            {() => <PreviewPane item={selected()} preview={farol.preview()} />}
          </Box>
        </Box>
        {footer}
      </Box>
    </Surface>
  );
}
