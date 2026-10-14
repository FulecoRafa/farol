// The settings window: aliases, shortcuts, links, plugins, the options and
// diagnostics, all from the service's `config` topic. Opened by `farol
// settings`, the "configurações do farol" row, or Ctrl+, in the launcher.

import { ActionButton } from "@kit/components/ActionButton";
import { Icon } from "@kit/components/Icon";
import { SectionTitle } from "@kit/components/SectionTitle";
import { Toggle } from "@kit/components/Toggle";
import * as farol from "../data/farol";
import { ItemIcon } from "../components/ItemIcon";

const WIDTH = 900;
const HEIGHT = 600;

const PAGES: [string, string, string][] = [
  ["geral", "gear-six", "geral"],
  ["aliases", "at", "pseudônimos"],
  ["hotkeys", "keyboard", "atalhos"],
  ["links", "link", "links rápidos"],
  ["plugins", "puzzle-piece", "plugins"],
  ["doctor", "stethoscope", "diagnóstico"],
];

/** A field with a caption: the text lives in `value`, Enter submits. */
function Field(p: { label: string; value: WritableSignal<string>; placeholder?: string; width?: number; onSubmit?: () => void }) {
  return (
    <Box direction="column" gap={4} width={p.width}>
      <Label text={p.label} size={10} color="$textFaint" maxWidth={p.width} />
      <Box width={p.width} height={34} padding={[0, 12]} radius={10} align="center" bg="$sunken" border="$surfaceBorder">
        <Input
          value={p.value}
          placeholder={p.placeholder ?? ""}
          size={12}
          onChange={(e) => p.value.set(e.value)}
          onSubmit={() => p.onSubmit?.()}
          onKey={(e) => e.key === "escape" && farol.hide()}
        />
      </Box>
    </Box>
  );
}

/** One saved thing: picture, what it is, where it goes, a remove button. */
function SavedRow(p: { icon: string; title: string; subtitle: string; tag: string; onRemove: () => void }) {
  return (
    <Box height={46} padding={[0, 12]} gap={12} align="center" radius={12} hoverBg="$hover">
      <ItemIcon item={/^(icon|file|ph):/.test(p.icon) ? { id: "x:x", title: p.title, icon: p.icon } : { id: "x:x", title: p.title, glyph: p.icon }} size={24} />
      <Box direction="column" gap={1} grow>
        <Label text={p.title} size={13} color="$textBright" maxWidth={420} />
        {p.subtitle ? <Label text={p.subtitle} size={11} color="$textFaint" maxWidth={420} /> : null}
      </Box>
      <Box height={24} padding={[0, 10]} radius={8} align="center" bg="$raised">
        <Label text={p.tag} size={11} color="$yellow" />
      </Box>
      <Box width={28} height={28} radius={8} align="center" justify="center" hoverBg="$redTint" tooltip="apagar" onClick={p.onRemove}>
        <Icon name="trash" size={15} color="$textMuted" />
      </Box>
    </Box>
  );
}

function Empty(p: { text: string }) {
  return (
    <Box padding={[10, 12]}>
      <Label text={p.text} size={12} color="$textFaint" wrap />
    </Box>
  );
}

export default function Settings() {
  const page = signal("geral");
  const message = signal({ text: "", error: false });
  let clear = () => {};
  const say = (text: string, error = false) => {
    clear();
    message.set({ text, error });
    clear = after(3000, () => message.set({ text: "", error: false }));
  };
  /** Run an edit; on success clear the fields and say `ok`. */
  const edit = (method: string, args: Record<string, unknown>, ok: string, fields: WritableSignal<string>[] = []) =>
    farol.edit(method, args, (error) => {
      if (error) return say(error, true);
      fields.forEach((f) => f.set(""));
      say(ok);
    });

  const option = (key: string) => farol.config().options[key];
  const words = (key: string) => {
    const v = option(key);
    return Array.isArray(v) ? v.join(" ") : String(v ?? "");
  };

  // ── geral ────────────────────────────────────────────────────────────

  const switchRow = (title: string, about: string, checked: () => boolean, set: (on: boolean) => void) => (
    <Box height={52} padding={[0, 12]} gap={12} align="center">
      <Box direction="column" gap={2} grow>
        <Label text={title} size={13} color="$textBright" />
        <Label text={about} size={11} color="$textFaint" />
      </Box>
      <Toggle checked={checked} onToggle={set} />
    </Box>
  );

  /** A command option ("ghostty -e"), saved as a list of words. */
  const commandField = (key: string, label: string, hint: string) => {
    const value = signal(words(key));
    return (
      <Field
        label={label}
        value={value}
        placeholder={hint}
        width={300}
        onSubmit={() => edit("option_set", { key, value: value.peek().split(/\s+/).filter(Boolean) }, `${label} salvo`)}
      />
    );
  };

  const general = () => {
    const search = signal(String(option("web.search") ?? ""));
    const limit = signal(String(option("clipboard.limit") ?? "500"));
    return (
      <Box direction="column" gap={6}>
        <SectionTitle label="inicialização" />
        {switchRow("iniciar no login", "roda o farol quando a sessão começa", () => farol.config().autostart, (on) =>
          edit("option_set", { key: "autostart", value: on }, on ? "o farol inicia no login" : "não inicia mais no login"),
        )}
        <SectionTitle label="busca" />
        {switchRow("arquivos na busca", "três letras já trazem arquivos da home", () => option("files.mixed") !== false, (on) =>
          edit("option_set", { key: "files.mixed", value: on }, "salvo"),
        )}
        {switchRow("buscar na web no fim", "uma linha para buscar o texto na web", () => option("web.fallback") !== false, (on) =>
          edit("option_set", { key: "web.fallback", value: on }, "salvo"),
        )}
        {switchRow("arquivos ocultos", "inclui pastas e arquivos que começam com ponto", () => option("files.hidden") === true, (on) =>
          edit("option_set", { key: "files.hidden", value: on }, "salvo"),
        )}
        <SectionTitle label="programas (enter salva)" />
        <Box gap={12} padding={[4, 12]}>
          {commandField("terminal", "terminal", "ghostty -e")}
          {commandField("editor", "editor", "ghostty -e hx {path}")}
        </Box>
        <Box gap={12} padding={[4, 12]}>
          {commandField("browser", "navegador (vazio: o padrão)", "helium")}
          {commandField("terminal.here", "terminal na pasta", "ghostty --working-directory={dir}")}
        </Box>
        <Box gap={12} padding={[4, 12]}>
          <Field label="busca na web ({query} é o texto)" value={search} width={300} onSubmit={() => edit("option_set", { key: "web.search", value: search.peek() }, "busca salva")} />
          <Field
            label="itens no clipboard"
            value={limit}
            width={160}
            onSubmit={() => edit("option_set", { key: "clipboard.limit", value: Number(limit.peek()) || 500 }, "limite salvo")}
          />
        </Box>
      </Box>
    );
  };

  // ── pseudônimos, atalhos, links ──────────────────────────────────────

  const aliases = () => {
    const name = signal("");
    const target = signal("");
    const add = () => edit("alias_add", { name: name.peek(), target: target.peek() }, `“${name.peek()}” criado`, [name, target]);
    const list = farol.config().aliases;
    return (
      <Box direction="column" gap={8}>
        <Box gap={10} align="end" padding={[0, 12]}>
          <Field label="nome" value={name} placeholder="zap" width={180} onSubmit={add} />
          <Field label="abre" value={target} placeholder="whatsapp" width={300} onSubmit={add} />
          <ActionButton label="criar" primary accent="$yellow" onClick={add} />
        </Box>
        <SectionTitle label={`${list.length} pseudônimos`} />
        {list.length === 0 ? <Empty text="nenhum ainda. no launcher, ctrl+a em qualquer linha também cria." /> : null}
        {list.map((a) => (
          <SavedRow key={a.name} icon={a.icon} title={a.title} subtitle={a.subtitle} tag={a.name} onRemove={() => edit("alias_remove", { name: a.name }, `“${a.name}” apagado`)} />
        ))}
      </Box>
    );
  };

  const hotkeys = () => {
    const keys = signal("");
    const target = signal("");
    const add = () => edit("hotkey_add", { keys: keys.peek(), target: target.peek() }, `${keys.peek()} criado`, [keys, target]);
    const cfg = farol.config();
    return (
      <Box direction="column" gap={8}>
        <Box gap={10} align="end" padding={[0, 12]}>
          <Field label="teclas" value={keys} placeholder="super+alt+w" width={170} onSubmit={add} />
          <Field label="faz: vazio abre o farol, um nome roda o item" value={target} placeholder="whatsapp" width={280} onSubmit={add} />
          <ActionButton label="criar" primary accent="$yellow" onClick={add} />
        </Box>
        <SectionTitle label="do farol" />
        {cfg.hotkeys.length === 0 ? <Empty text="nenhum ainda. no launcher, ctrl+s em qualquer linha também cria." /> : null}
        {cfg.hotkeys.map((h) => (
          <SavedRow
            key={h.keys}
            icon={h.icon}
            title={h.title}
            subtitle={h.replaced ? `${h.subtitle ? h.subtitle + " · " : ""}no lugar de “${h.replaced}”` : h.subtitle}
            tag={h.keys}
            onRemove={() => edit("hotkey_remove", { keys: h.keys }, `${h.keys} apagado`)}
          />
        ))}
        <SectionTitle label={`do desktop (${cfg.desktop.length})`} />
        <Box direction="column" maxHeight={220} scroll>
          {cfg.desktop.map((b, i) => (
            <Box key={i} height={30} padding={[0, 12]} gap={12} align="center">
              <Box width={190}>
                <Label text={b.keys} size={11} color="$textMuted" />
              </Box>
              <Label text={b.title} size={12} color="$text" maxWidth={560} />
            </Box>
          ))}
        </Box>
      </Box>
    );
  };

  const links = () => {
    const name = signal("");
    const url = signal("");
    const add = () => edit("link_add", { name: name.peek(), url: url.peek() }, `link “${name.peek()}” criado`, [name, url]);
    const list = farol.config().links;
    return (
      <Box direction="column" gap={8}>
        <Box gap={10} align="end" padding={[0, 12]}>
          <Field label="nome" value={name} placeholder="gh" width={140} onSubmit={add} />
          <Field label="endereço ({query} recebe o texto depois do nome)" value={url} placeholder="https://github.com/search?q={query}" width={340} onSubmit={add} />
          <ActionButton label="criar" primary accent="$yellow" onClick={add} />
        </Box>
        <SectionTitle label={`${list.length} links`} />
        {list.length === 0 ? <Empty text="nenhum ainda. no launcher: link gh https://github.com/search?q={query}" /> : null}
        {list.map((l) => (
          <SavedRow key={l.name} icon="ph:link" title={l.url} subtitle="" tag={l.name} onRemove={() => edit("link_remove", { name: l.name }, `link “${l.name}” apagado`)} />
        ))}
      </Box>
    );
  };

  const plugins = () => {
    const list = farol.config().plugins;
    return (
      <Box direction="column" gap={8}>
        <SectionTitle label="plugins externos" />
        {list.length === 0 ? <Empty text={`nenhum. um plugin é um serviço com query e activate (veja examples/hello-plugin.py); declare com farol set plugins '[{"name": "hello", "prefix": "oi "}]' e reinicie o farol.`} /> : null}
        {list.map((pl) => (
          <Box key={pl.name} height={46} padding={[0, 12]} gap={12} align="center">
            <Icon name="puzzle-piece" size={20} color="$textMuted" />
            <Box direction="column" gap={1} grow>
              <Label text={pl.title || pl.name} size={13} color="$textBright" />
              <Label text={`prefixo “${pl.prefix ?? ""}” · ${pl.mixed ?? 0} na busca normal`} size={11} color="$textFaint" />
            </Box>
            <Label text={pl.online ? "rodando" : "parado"} size={11} color={pl.online ? "$green" : "$red"} />
          </Box>
        ))}
        <SectionTitle label="embutidos" />
        <Empty text="apps · calculadora (=) · emoji (:) · arquivos (/) · clipboard (>) · janelas (!) · timers · web (?) · atalhos · pseudônimos" />
      </Box>
    );
  };

  const doctor = () => {
    const s = signal<Record<string, any> | null>(null);
    const load = () => farol.status((v) => s.set(v));
    load();
    const line = (k: string, v: string) => (
      <Box height={24} padding={[0, 12]} gap={12} align="center">
        <Box width={130}>
          <Label text={k} size={11} color="$textFaint" />
        </Box>
        <Label text={v} size={12} color="$text" maxWidth={600} />
      </Box>
    );
    return <Box direction="column">{() => {
      const v = s();
      if (!v) return <Empty text="lendo…" />;
      const d = v.database;
      return (
        <Box direction="column" gap={4}>
          <SectionTitle label={`farol ${v.version} · ${v.os}`} action="atualizar" onAction={load} />
          {line("memória", `${Math.round(v.memory_kb / 1024)} MB no núcleo`)}
          {line("banco", `${d.path} · ${Math.round(d.bytes / 1024)} KB`)}
          {line("guardado", `${d.aliases} pseudônimos · ${d.hotkeys} atalhos · ${d.links} links · ${d.clipboard} no clipboard · ${d.timers} timers`)}
          {line("projeto", v.project ?? "—")}
          {line("atalhos / janelas", `${v.hotkeys ? "sim" : "não"} / ${v.windows ? "sim" : "não"}`)}
          {line("devtools", "farol dev abre uma cópia com o devtools; farol doctor no terminal")}
          <SectionTitle label="busca por provider (ms)  última · máx · média · vezes" />
          {(v.timings as any[]).map((t) => (
            <Box key={t.provider} height={22} padding={[0, 12]} gap={12} align="center">
              <Box width={130}>
                <Label text={t.provider} size={11} color="$textMuted" />
              </Box>
              <Label text={`${t.last.toFixed(2)} · ${t.max.toFixed(2)} · ${t.avg.toFixed(2)} · ${t.count}`} size={11} color={t.max > 16 ? "$orange" : "$text"} />
            </Box>
          ))}
        </Box>
      );
    }}</Box>;
  };

  const content = () => {
    switch (page()) {
      case "aliases": return aliases();
      case "hotkeys": return hotkeys();
      case "links": return links();
      case "plugins": return plugins();
      case "doctor": return doctor();
      default: return general();
    }
  };

  return (
    <Surface
      layer="overlay"
      anchor="top"
      width={WIDTH}
      height={HEIGHT}
      margin={[100, 0, 0, 0]}
      keyboard="exclusive"
      visible={() => farol.open() && farol.view() === "settings"}
      onDismiss={farol.hide}
    >
      <Box grow radius={24} bg="$surface" border="$surfaceBorder">
        <Box width={200} direction="column" gap={4} padding={[22, 12]} bg="$sunken" radius={[24, 0, 0, 24]}>
          <Box gap={10} align="center" padding={[0, 10, 14, 10]}>
            <Icon name="lighthouse" size={20} color="$yellow" />
            <Label text="farol" size={15} weight="semibold" color="$textBright" />
          </Box>
          {PAGES.map(([id, icon, title]) => (
            <Box
              key={id}
              height={36}
              padding={[0, 10]}
              gap={10}
              align="center"
              radius={10}
              bg={() => (page() === id ? "$raised" : "#00000000")}
              hoverBg="$hover"
              onClick={() => page.set(id)}
            >
              <Icon name={icon} size={16} color={() => (page() === id ? "$yellow" : "$textMuted")} />
              <Label text={title} size={12} color={() => (page() === id ? "$textBright" : "$text")} />
            </Box>
          ))}
          <Box grow />
          <Box padding={[0, 10]} onClick={farol.hide} hoverOpacity={0.8}>
            <Label text="fechar · esc" size={11} color="$textFaint" />
          </Box>
        </Box>
        <Box grow direction="column">
          <Box grow direction="column" padding={[22, 24, 12, 24]} scroll>
            {content}
          </Box>
          <Box height={34} padding={[0, 24]} align="center">
            {() => {
              const m = message();
              return m.text ? <Label text={m.text} size={11} color={m.error ? "$red" : "$green"} /> : null;
            }}
          </Box>
        </Box>
      </Box>
    </Surface>
  );
}
