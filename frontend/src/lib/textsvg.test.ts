import { describe, expect, it } from "vitest";

import { composeSvg } from "./textsvg";

const base = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><defs></defs><path d="M0 0h10v10z" fill="#fff"/></svg>';
const group = '<g id="text"><path d="M1 1h2" fill="#000" data-line="a"/></g>';

describe("composeSvg", () => {
  it("inserts the group before the closing tag", () => {
    expect(composeSvg(base, group)).toBe(base.replace("</svg>", `${group}</svg>`));
  });
  it("keeps the group a sibling of an upsample root group", () => {
    const up = '<svg viewBox="0 0 10 10"><g transform="scale(0.5)"><path d="M0 0h20v20z"/></g></svg>';
    const out = composeSvg(up, group);
    expect(out).toBe(`<svg viewBox="0 0 10 10"><g transform="scale(0.5)"><path d="M0 0h20v20z"/></g>${group}</svg>`);
  });
  it("leaves the base alone without a group or a closing tag", () => {
    expect(composeSvg(base, null)).toBe(base);
    expect(composeSvg(base, "")).toBe(base);
    expect(composeSvg("<svg>", group)).toBe("<svg>");
  });
  it("leaves the hidden text out when the setting is off, and keeps the glyphs", () => {
    const withText = '<g id="text"><path d="M1 1h2" fill="#000" data-line="a"/><text x="1 2" y="3" font-family="A &amp; B" fill="none" data-line="a">A &lt;b&gt;</text><path d="M4 4h1" data-line="b"/><text x="4" y="5" data-line="b">C</text></g>';
    expect(composeSvg(base, withText, { hiddenText: true })).toBe(base.replace("</svg>", `${withText}</svg>`));
    expect(composeSvg(base, withText, { hiddenText: false })).toBe(base.replace("</svg>", '<g id="text"><path d="M1 1h2" fill="#000" data-line="a"/><path d="M4 4h1" data-line="b"/></g></svg>'));
  });
});
