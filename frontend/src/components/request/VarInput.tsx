import { useEffect, useId, useLayoutEffect, useRef, useState, type InputHTMLAttributes, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import { copyResolved } from "../../lib/copyResolved";
import { copiedValueNote } from "../../lib/varText";
import { explain, tokenize, type VarToken } from "../../lib/varTokens";
import { useTabs } from "../../store/tabs";
import { useVars } from "../../store/vars";
import { useWorkspace } from "../../store/workspace";
import { useCopy } from "../response/useCopy";
import { VarHover } from "./VarHover";

type Props = InputHTMLAttributes<HTMLInputElement> & {
  /** The request tab whose variables explain the `{{tokens}}`. */
  tabKey: string;
  /** Class of the wrapper (layout: `flex: 1` in the URL bar). */
  wrapperClassName?: string;
};

/** How long the caret stays in a token before its card shows. */
const CARET_DELAY = 500;
/** How long the card waits for the pointer to move onto it. */
const LEAVE_DELAY = 150;

/** ⌘/Ctrl+Shift+C. */
const isCopyShortcut = (e: KeyboardEvent<HTMLInputElement>) =>
  (e.metaKey || e.ctrlKey) && e.shiftKey && !e.altKey && e.key.toLowerCase() === "c";

/**
 * A plain `<input>` with `{{variables}}` underlined by class (defined, set later,
 * undefined) on an `aria-hidden` mirror behind it, and a hover card for the token under
 * the pointer or the caret. The input keeps its value, selection and events.
 */
export function VarInput({ tabKey, wrapperClassName, ...props }: Props) {
  const vars = useVars((s) => s.byTab[tabKey]);
  const collectionId = useTabs((s) => s.tabs.find((t) => t.key === tabKey)?.collectionId);
  const env = useWorkspace((s) => (collectionId ? (s.activeEnv[collectionId] ?? null) : null));
  const input = useRef<HTMLInputElement>(null);
  const mirror = useRef<HTMLDivElement>(null);
  const caretTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const leaveTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const { note, say } = useCopy();
  const [shown, setShown] = useState<{ token: VarToken; anchor: { left: number; bottom: number } } | null>(null);
  const tooltipId = useId();
  const text = String(props.value ?? "");
  const tokens = tokenize(text);

  // The mirror copies the input's box so the underlines sit under the characters.
  useLayoutEffect(() => {
    const el = input.current;
    const m = mirror.current;
    if (!el || !m) return;
    const cs = getComputedStyle(el);
    for (const p of ["paddingLeft", "paddingRight", "paddingTop", "paddingBottom", "borderLeftWidth", "borderTopWidth", "fontFamily", "fontSize", "fontWeight", "letterSpacing", "lineHeight"] as const) {
      m.style[p] = cs[p];
    }
    m.scrollLeft = el.scrollLeft;
  });
  useEffect(
    () => () => {
      clearTimeout(caretTimer.current);
      clearTimeout(leaveTimer.current);
    },
    [],
  );

  const copyValue = (name: string) => void copyResolved(tabKey, `{{${name}}}`, copiedValueNote(name)).then(say);
  const copyable = (name: string) => explain(name, vars)?.kind === "defined";

  const tokenAt = (offset: number) => tokens.find((t) => offset > t.start && offset < t.end);
  const spanBox = (token: VarToken) => {
    const span = mirror.current?.querySelector<HTMLElement>(`[data-start="${token.start}"]`);
    const r = span?.getBoundingClientRect() ?? input.current?.getBoundingClientRect();
    return { left: r?.left ?? 0, bottom: r?.bottom ?? 0 };
  };

  const onCaret = () => {
    clearTimeout(caretTimer.current);
    const el = input.current;
    if (!el || el.selectionStart !== el.selectionEnd || el.selectionStart === null) return;
    const token = tokenAt(el.selectionStart);
    if (!token) {
      if (shown) setShown(null);
      return;
    }
    caretTimer.current = setTimeout(() => setShown({ token, anchor: spanBox(token) }), CARET_DELAY);
  };

  const onMouseMove = (e: MouseEvent<HTMLInputElement>) => {
    props.onMouseMove?.(e);
    clearTimeout(leaveTimer.current);
    const spans = mirror.current?.querySelectorAll<HTMLElement>("[data-start]") ?? [];
    for (const span of spans) {
      const r = span.getBoundingClientRect();
      if (e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom) {
        const token = tokens.find((t) => String(t.start) === span.dataset.start);
        if (token && shown?.token.start !== token.start) setShown({ token, anchor: { left: r.left, bottom: r.bottom } });
        return;
      }
    }
    if (shown) setShown(null);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    const el = input.current;
    if (isCopyShortcut(e) && el && el.selectionStart !== null && el.selectionStart === el.selectionEnd) {
      const token = tokenAt(el.selectionStart);
      if (token && copyable(token.name)) {
        e.preventDefault();
        copyValue(token.name);
        return;
      }
    }
    if (e.key === "Escape" && shown) {
      // Handled here: Esc must not also cancel a running send.
      e.preventDefault();
      setShown(null);
    }
    props.onKeyDown?.(e);
  };

  const info = shown && vars ? explain(shown.token.name, vars) : null;
  const pieces: ReactNode[] = [];
  let at = 0;
  for (const t of tokens) {
    pieces.push(text.slice(at, t.start));
    const cls = vars ? explain(t.name, vars)?.kind : undefined;
    pieces.push(
      <span key={t.start} data-start={t.start} className={cls ? `var var-${cls.toLowerCase()}` : "var"}>
        {text.slice(t.start, t.end)}
      </span>,
    );
    at = t.end;
  }
  pieces.push(text.slice(at));

  return (
    <span className={`var-input ${wrapperClassName ?? ""}`.trim()}>
      <div ref={mirror} className="var-mirror" aria-hidden="true">
        {pieces}
      </div>
      <input
        {...props}
        ref={input}
        aria-describedby={info ? tooltipId : props["aria-describedby"]}
        onKeyDown={onKeyDown}
        onKeyUp={(e) => {
          props.onKeyUp?.(e);
          onCaret();
        }}
        onSelect={(e) => {
          props.onSelect?.(e);
          onCaret();
        }}
        onClick={(e) => {
          props.onClick?.(e);
          onCaret();
        }}
        onBlur={(e) => {
          props.onBlur?.(e);
          clearTimeout(caretTimer.current);
          setShown(null);
        }}
        onMouseMove={onMouseMove}
        onMouseLeave={(e) => {
          props.onMouseLeave?.(e);
          // Wait a moment: the pointer may be on its way to the card.
          clearTimeout(leaveTimer.current);
          leaveTimer.current = setTimeout(() => setShown(null), LEAVE_DELAY);
        }}
        onScroll={(e) => {
          props.onScroll?.(e);
          if (mirror.current) mirror.current.scrollLeft = e.currentTarget.scrollLeft;
        }}
      />
      <span className="sr-only" aria-live="polite">
        {note}
      </span>
      {shown && info && (
        <VarHover
          id={tooltipId}
          name={shown.token.name}
          info={info}
          env={env}
          anchor={shown.anchor}
          note={note}
          onCopy={info.kind === "defined" ? () => copyValue(shown.token.name) : undefined}
          onMouseEnter={() => clearTimeout(leaveTimer.current)}
          onMouseLeave={() => {
            clearTimeout(leaveTimer.current);
            leaveTimer.current = setTimeout(() => setShown(null), LEAVE_DELAY);
          }}
        />
      )}
    </span>
  );
}
