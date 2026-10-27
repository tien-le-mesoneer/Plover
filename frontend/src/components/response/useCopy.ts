import { useEffect, useRef, useState } from "react";

/** Copies text to the clipboard and keeps a short note ("Copied") for an aria-live region. */
export function useCopy(ms = 1500): {
  note: string | null;
  copy: (text: string, done?: string) => Promise<void>;
  /** Shows a note without touching the clipboard. */
  say: (note: string) => void;
} {
  const [note, setNote] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const say = (text: string) => {
    setNote(text);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setNote(null), ms);
  };
  const copy = async (text: string, done = "Copied") => {
    try {
      await navigator.clipboard.writeText(text);
      say(done);
    } catch {
      say("Could not copy");
    }
  };
  return { note, copy, say };
}
