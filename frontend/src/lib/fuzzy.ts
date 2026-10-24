/** Quick-open matching: in-house subsequence scoring, no dependency. */

export interface QuickItem {
  collectionId: string;
  item: string;
  method: string;
  name: string;
  /** Folder names from the collection root, `/`-separated (`""` at the root). */
  path: string;
  collection: string;
}

/** Lower case without diacritics (`Đăng` → `dang`), so matching ignores both. */
export function fold(s: string): string {
  return s.normalize("NFD").replace(/\p{M}/gu, "").replace(/[đĐ]/g, "d").toLowerCase();
}

interface Folded {
  name: string;
  full: string;
}

const folded = new WeakMap<QuickItem, Folded>();

function foldedOf(item: QuickItem): Folded {
  let f = folded.get(item);
  if (!f) {
    const full = [item.collection, item.path, item.name].filter(Boolean).join(" / ");
    f = { name: fold(item.name), full: fold(full) };
    folded.set(item, f);
  }
  return f;
}

const isWordStart = (text: string, i: number) => i === 0 || /[\s/_\-.]/.test(text[i - 1]!);

/**
 * Greedy subsequence match of `query` (spaces ignored) in `text`; `null` when it is not a
 * subsequence. Each character scores 1, plus 4 at a word start and 3 right after the
 * previous match; gaps cost a little, and so does a longer text.
 */
function match(query: string, text: string): number | null {
  let at = 0;
  let total = 0;
  let previous = -2;
  for (const ch of query) {
    if (ch === " ") continue;
    let found = text.indexOf(ch, at);
    if (found < 0) return null;
    // Prefer a later word start for this character when one exists before the next gap.
    if (!isWordStart(text, found) && found !== previous + 1) {
      for (let j = found + 1; j < text.length; j++) {
        if (text[j] === ch && isWordStart(text, j)) {
          found = j;
          break;
        }
      }
    }
    total += 1;
    if (isWordStart(text, found)) total += 4;
    if (found === previous + 1) total += 3;
    else if (previous >= 0) total -= Math.min(3, (found - previous - 1) * 0.1);
    previous = found;
    at = found + 1;
  }
  return total - text.length * 0.01;
}

/** Higher is better; matches in the name beat matches that need the folder path. */
export function score(query: string, item: QuickItem): number | null {
  const q = fold(query).trim();
  return scoreFolded(q, item);
}

function scoreFolded(q: string, item: QuickItem): number | null {
  const f = foldedOf(item);
  const inName = match(q, f.name);
  if (inName !== null) return inName + 100;
  return match(q, f.full);
}

/** The best `limit` matches; equal scores keep the input (tree) order. */
export function rank(query: string, items: QuickItem[], limit = 50): QuickItem[] {
  const q = fold(query).trim();
  const scored: { item: QuickItem; score: number; index: number }[] = [];
  items.forEach((item, index) => {
    const s = scoreFolded(q, item);
    if (s !== null) scored.push({ item, score: s, index });
  });
  scored.sort((a, b) => b.score - a.score || a.index - b.index);
  return scored.slice(0, limit).map((s) => s.item);
}
