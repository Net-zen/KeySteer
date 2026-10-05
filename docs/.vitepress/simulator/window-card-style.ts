import type { ConfigDocument } from '../config-studio/document.ts'

/** Shared selected-card style for the swatches and interactive window preview. */
export function selectedCardStyle(card: ConfigDocument, appearance: string) {
  const color = (value: unknown, fallback: string): string => typeof value === 'string' ? value
    : (value as Record<string, string> | undefined)?.[appearance] ?? fallback
  return {
    background: color(card.selected_background_color, appearance === 'dark' ? '#284D44FF' : '#E8F6F0FF'),
    border: color(card.selected_border_color, appearance === 'dark' ? '#85CDB8FF' : '#60B49CFF'),
    borderWidth: Number(card.selected_border_width ?? 1.5),
  }
}
