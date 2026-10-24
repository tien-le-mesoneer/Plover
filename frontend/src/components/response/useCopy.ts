import { useEffect, useRef, useState } from "react";

/** Copies text to the clipboard and keeps a short note ("Copied") for an aria-live region. */
export function useCopy(ms = 1500): { note: string | null; copy: (text: string, done?: string) => Promise<void> } {
  const [note, setNote] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const copy = async (text: string, done = "Copied") => {
    try {
      await navigator.clipboard.writeText(text);
      setNote(done);
    } catch {
      setNote("Could not copy");
    }
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setNote(null), ms);
  };
  return { note, copy };
}
