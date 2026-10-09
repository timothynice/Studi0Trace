/** The trace's SVG with the text group inside it: before the closing tag, a sibling of everything else (an upsampled trace's own root group included). */
export function composeSvg(base: string, group: string | null | undefined): string {
  if (!group) return base;
  const at = base.lastIndexOf("</svg>");
  if (at < 0) return base;
  return base.slice(0, at) + group + base.slice(at);
}
