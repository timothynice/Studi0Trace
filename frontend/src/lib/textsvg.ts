export interface ComposeOptions {
  /** Settings ▸ Text ▸ hidden text: the app always writes the invisible `<text>` beside each line's glyphs; off, it is left out here. */
  hiddenText: boolean;
}

/** Every `<text …>…</text>` of the group (the app escapes `<` in the string, so none ends early). */
const HIDDEN_TEXT = /<text\b[^>]*>[\s\S]*?<\/text>/g;

/** The trace's SVG with the text group inside it: before the closing tag, a sibling of everything else (an upsampled trace's own root group included). */
export function composeSvg(base: string, group: string | null | undefined, { hiddenText }: ComposeOptions = { hiddenText: true }): string {
  if (!group) return base;
  const at = base.lastIndexOf("</svg>");
  if (at < 0) return base;
  const g = hiddenText ? group : group.replace(HIDDEN_TEXT, "");
  return base.slice(0, at) + g + base.slice(at);
}
