import type { Item, Preview } from "../data/farol";
import { ItemIcon } from "./ItemIcon";

/** Label/value lines under a preview. */
function Facts(p: { facts: [string, string][] }) {
  return (
    <Box direction="column" gap={6}>
      {p.facts.map(([k, v]) => (
        <Box key={k} gap={10}>
          <Label text={k} size={11} color="$textFaint" />
          <Label text={v} size={11} color="$textMuted" maxWidth={300} />
        </Box>
      ))}
    </Box>
  );
}

/**
 * The right side: what the service previewed for the selected row, or
 * the row itself in big while the preview is on its way (or has none).
 */
export function PreviewPane(p: { item: Item | null; preview: Preview | null }) {
  const { item, preview } = p;
  if (!item) return <Box grow />;
  const ready = preview && preview.id === item.id ? preview : null;
  if (ready?.kind === "image") {
    return (
      <Box grow direction="column" gap={12} align="center" padding={[8, 4]}>
        <Image src={ready.src} size={260} radius={12} />
        <Label text={ready.title} size={13} color="$textBright" maxWidth={330} />
        {ready.facts ? <Facts facts={ready.facts} /> : null}
      </Box>
    );
  }
  return (
    <Box grow direction="column" gap={14} padding={[8, 4]}>
      <Box gap={14} align="center">
        <ItemIcon item={item} size={item.glyph ? 72 : 48} />
        <Box direction="column" gap={2}>
          <Label text={ready?.kind === "text" ? ready.title : item.title} size={15} weight="semibold" color="$textBright" maxWidth={260} />
          {item.subtitle ? <Label text={item.subtitle} size={11} color="$textFaint" maxWidth={260} /> : null}
        </Box>
      </Box>
      {ready?.kind === "text" && ready.text ? (
        <Box radius={10} bg="$sunken" padding={12} maxHeight={220} scroll>
          <Label text={ready.text} size={12} color="$text" wrap />
        </Box>
      ) : null}
      {ready?.kind === "text" && ready.facts ? <Facts facts={ready.facts} /> : null}
    </Box>
  );
}
