// One tooltip for all charts, placed at the pointer.

export interface TipLine {
  label: string;
  value: string;
  /** A colour swatch before the label. */
  color?: string;
}

export const tip = $state({
  visible: false,
  x: 0,
  y: 0,
  title: "",
  lines: [] as TipLine[],
});

export function showTip(event: PointerEvent | FocusEvent, title: string, lines: TipLine[]) {
  let x: number;
  let y: number;
  if ("clientX" in event && event.clientX) {
    x = event.clientX;
    y = event.clientY;
  } else {
    const box = (event.currentTarget as Element).getBoundingClientRect();
    x = box.left + box.width / 2;
    y = box.top;
  }
  Object.assign(tip, { visible: true, x, y, title, lines });
}

export function hideTip() {
  tip.visible = false;
}
