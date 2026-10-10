import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import { EMPTY_INSPECTOR, Inspector, tinyShapes, type InspectorState } from "./Inspector";
import { parseSvg } from "@/lib/svgdoc";

const SVG = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 48">
<rect x="0" y="0" width="64" height="48" fill="#eeeeee"/>
<path d="M8 8L24 8L24 20L8 20Z" fill="#2a9d8f"/>
<rect x="60" y="44" width="2" height="2" fill="#ff0000"/></svg>`;

const doc = parseSvg(SVG)!;
const state = (patch: Partial<InspectorState> = {}) => ({ ...EMPTY_INSPECTOR, open: true, ...patch });

test("lists every shape with its anchor count", () => {
  render(<Inspector doc={doc} bytes={512} state={state()} onChange={() => {}} onToggle={() => {}} />);
  expect(screen.getByText("Rect 1")).toBeInTheDocument();
  expect(screen.getByText("Path 2")).toBeInTheDocument();
  expect(screen.getByText("Rect 3")).toBeInTheDocument();
  expect(screen.getByText("12")).toBeInTheDocument(); // 4 + 4 + 4 anchors
});

test("hiding a shape reports it back", () => {
  const onToggle = vi.fn();
  render(<Inspector doc={doc} bytes={512} state={state()} onChange={() => {}} onToggle={onToggle} />);
  fireEvent.click(screen.getByRole("button", { name: "Hide Path 2" }));
  expect(onToggle).toHaveBeenCalledWith(1);
});

test("the speck threshold picks out only the small shapes", () => {
  // The 2x2 speck goes; the 16x12 path and the full-canvas rect stay.
  expect(tinyShapes(doc, 0)).toEqual([]);
  expect(tinyShapes(doc, 10)).toEqual([2]);
  expect(tinyShapes(doc, 200)).toEqual([1, 2]);
});

test("says what the cleanup will actually drop", () => {
  render(<Inspector doc={doc} bytes={512} state={state({ minArea: 10 })} onChange={() => {}} onToggle={() => {}} />);
  expect(screen.getByText(/1 shape will be left out/)).toBeInTheDocument();
});

test("the display rail toggles anchors and outlines, and is off until there is a vector", () => {
  const onChange = vi.fn();
  const { rerender } = render(<Inspector doc={doc} bytes={512} state={state({ outlines: true })} onChange={onChange} onToggle={() => {}} />);
  expect(screen.getByRole("button", { name: "Outlines" })).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(screen.getByRole("button", { name: "Anchor points" }));
  expect(onChange).toHaveBeenCalledWith({ points: true });
  rerender(<Inspector doc={null} bytes={0} state={state()} onChange={onChange} onToggle={() => {}} />);
  expect(screen.getByRole("button", { name: "Anchor points" })).toBeDisabled();
  expect(screen.getByText("Trace this image to inspect it.")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Layers" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Clean up" })).toBeNull();
});

test("Text, Layers and Clean up are three disclosure sections, open to begin with", () => {
  render(<Inspector doc={doc} bytes={512} text={<p>the text body</p>} state={state()} onChange={() => {}} onToggle={() => {}} />);
  for (const name of ["Text", "Layers", "Clean up"]) expect(screen.getByRole("button", { name })).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByText("the text body")).toBeInTheDocument();
  expect(screen.getByText("Rect 1")).toBeInTheDocument();
  expect(screen.getByLabelText("Drop shapes smaller than")).toBeInTheDocument();
});

test("each header opens and closes its own body and reports the sections", () => {
  const onChange = vi.fn();
  const closed = { text: false, layers: false, cleanup: false };
  const { rerender } = render(<Inspector doc={doc} bytes={512} text={<p>the text body</p>} state={state()} onChange={onChange} onToggle={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: "Text" }));
  expect(onChange).toHaveBeenLastCalledWith({ sections: { text: false, layers: true, cleanup: true } });
  fireEvent.click(screen.getByRole("button", { name: "Layers" }));
  expect(onChange).toHaveBeenLastCalledWith({ sections: { text: true, layers: false, cleanup: true } });
  fireEvent.click(screen.getByRole("button", { name: "Clean up" }));
  expect(onChange).toHaveBeenLastCalledWith({ sections: { text: true, layers: true, cleanup: false } });
  rerender(<Inspector doc={doc} bytes={512} text={<p>the text body</p>} state={state({ sections: closed })} onChange={onChange} onToggle={() => {}} />);
  expect(screen.getByRole("button", { name: "Layers" })).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByText("the text body")).toBeNull();
  expect(screen.queryByText("Rect 1")).toBeNull();
  expect(screen.queryByLabelText("Drop shapes smaller than")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Text" }));
  expect(onChange).toHaveBeenLastCalledWith({ sections: { text: true, layers: false, cleanup: false } });
});

test("the Text section shows before a trace, when there is only a plan to show", () => {
  render(<Inspector doc={null} bytes={0} text={<p>the text body</p>} state={state()} onChange={() => {}} onToggle={() => {}} />);
  expect(screen.getByRole("button", { name: "Text" })).toBeInTheDocument();
  expect(screen.getByText("the text body")).toBeInTheDocument();
  expect(screen.getByText("Trace this image to inspect it.")).toBeInTheDocument();
});

test("without a text body there is no Text section", () => {
  render(<Inspector doc={doc} bytes={512} state={state()} onChange={() => {}} onToggle={() => {}} />);
  expect(screen.queryByRole("button", { name: "Text" })).toBeNull();
});
