import { Icon } from "@kit/components/Icon";
import type { Item } from "../data/farol";

/** Each provider's glyph when a row brings no icon of its own. */
const FALLBACK: Record<string, string> = {
  apps: "app-window",
  calc: "calculator",
  clipboard: "clipboard-text",
  emoji: "smiley",
  farol: "lighthouse",
};

/**
 * A row's picture: an emoji, a theme icon or file, or a Phosphor glyph,
 * centered in a square of `size` so the titles beside it line up.
 */
export function ItemIcon(p: { item: Item; size: number; color?: Color }) {
  const { item, size } = p;
  const icon = item.icon ?? "";
  const name = icon.startsWith("ph:") ? icon.slice(3) : FALLBACK[item.id.split(":")[0]] ?? "circle";
  return (
    <Box width={size} height={size} align="center" justify="center">
      {item.glyph
        ? <Label text={item.glyph} size={Math.round(size * 0.8)} />
        : icon.startsWith("icon:") || icon.startsWith("file:")
          ? <Image src={icon} size={size} />
          : <Icon name={name} size={Math.round(size * 0.8)} color={p.color ?? "$textMuted"} />}
    </Box>
  );
}
