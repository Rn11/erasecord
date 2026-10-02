// A picture of one year on Discord to share: the year's numbers, the
// busiest servers and the favourite emoji. DMs are left out, so the card
// names no one.

export interface YearCard {
  title: string;
  /** Label and value, drawn as tiles of two columns. */
  figures: [string, string][];
  serversLabel: string;
  servers: string[];
  emojiLabel: string;
  emoji: string[];
  footer: string;
}

const WIDTH = 1080;
const HEIGHT = 1350;
const PAD = 80;
const FONT = `system-ui, -apple-system, "Segoe UI", "Noto Sans", sans-serif`;
const EMOJI_FONT = `"Noto Color Emoji", "Apple Color Emoji", "Segoe UI Emoji", sans-serif`;

/** Cuts `text` with an ellipsis so it fits into `width`. */
function fit(ctx: CanvasRenderingContext2D, text: string, width: number): string {
  if (ctx.measureText(text).width <= width) return text;
  let chars = [...text];
  while (chars.length > 1 && ctx.measureText(`${chars.join("")}…`).width > width) chars = chars.slice(0, -1);
  return `${chars.join("")}…`;
}

export function drawYearCard(card: YearCard): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = WIDTH;
  canvas.height = HEIGHT;
  const ctx = canvas.getContext("2d")!;

  const background = ctx.createLinearGradient(0, 0, WIDTH, HEIGHT);
  background.addColorStop(0, "#1e1f4b");
  background.addColorStop(1, "#5865f2");
  ctx.fillStyle = background;
  ctx.fillRect(0, 0, WIDTH, HEIGHT);

  ctx.fillStyle = "#ffffff";
  ctx.textBaseline = "top";
  ctx.font = `700 64px ${FONT}`;
  ctx.fillText(fit(ctx, card.title, WIDTH - 2 * PAD), PAD, PAD);

  const tileWidth = (WIDTH - 2 * PAD - 32) / 2;
  const tileHeight = 150;
  let y = PAD + 130;
  card.figures.forEach(([label, value], i) => {
    const x = PAD + (i % 2) * (tileWidth + 32);
    const top = y + Math.floor(i / 2) * (tileHeight + 28);
    ctx.fillStyle = "rgba(255, 255, 255, 0.12)";
    ctx.beginPath();
    ctx.roundRect(x, top, tileWidth, tileHeight, 24);
    ctx.fill();
    ctx.fillStyle = "rgba(255, 255, 255, 0.75)";
    ctx.font = `500 28px ${FONT}`;
    ctx.fillText(fit(ctx, label, tileWidth - 56), x + 28, top + 26);
    ctx.fillStyle = "#ffffff";
    ctx.font = `700 46px ${FONT}`;
    ctx.fillText(fit(ctx, value, tileWidth - 56), x + 28, top + 70);
  });
  y += Math.ceil(card.figures.length / 2) * (tileHeight + 28) + 30;

  if (card.servers.length) {
    ctx.fillStyle = "rgba(255, 255, 255, 0.75)";
    ctx.font = `500 30px ${FONT}`;
    ctx.fillText(card.serversLabel, PAD, y);
    y += 50;
    ctx.fillStyle = "#ffffff";
    ctx.font = `600 40px ${FONT}`;
    card.servers.forEach((name, i) => {
      ctx.fillText(fit(ctx, `${i + 1}. ${name}`, WIDTH - 2 * PAD), PAD, y);
      y += 58;
    });
    y += 30;
  }

  if (card.emoji.length) {
    ctx.fillStyle = "rgba(255, 255, 255, 0.75)";
    ctx.font = `500 30px ${FONT}`;
    ctx.fillText(card.emojiLabel, PAD, y);
    y += 50;
    ctx.font = `72px ${EMOJI_FONT}`;
    card.emoji.forEach((emoji, i) => ctx.fillText(emoji, PAD + i * 120, y));
  }

  ctx.fillStyle = "rgba(255, 255, 255, 0.6)";
  ctx.font = `500 26px ${FONT}`;
  ctx.textBaseline = "bottom";
  ctx.fillText(card.footer, PAD, HEIGHT - PAD + 20);
  return canvas;
}

export async function pngBytes(canvas: HTMLCanvasElement): Promise<number[]> {
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
  if (!blob) throw new Error("could not draw the picture");
  return Array.from(new Uint8Array(await blob.arrayBuffer()));
}
