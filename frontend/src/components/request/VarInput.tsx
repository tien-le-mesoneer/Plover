import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type InputHTMLAttributes,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
} from "react";
import { copyResolved } from "../../lib/copyResolved";
import { acceptSuggestion, openToken, suggestions, type VarSuggestion } from "../../lib/varSuggest";
import { copiedValueNote } from "../../lib/varText";
import { explain, tokenize, type VarToken } from "../../lib/varTokens";
import { useTabs } from "../../store/tabs";
import { useVars } from "../../store/vars";
import { useWorkspace } from "../../store/workspace";
import { useCopy } from "../response/useCopy";
import { VarHover } from "./VarHover";
import { VarSuggest } from "./VarSuggest";

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

type Anchor = { left: number; bottom: number };

/** Sets an input's value the way typing does, so React's `onChange` sees it. */
function typeValue(el: HTMLInputElement, value: string) {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(el, value);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

/**
 * A plain `<input>` with `{{variables}}` underlined by class (defined, set later,
 * undefined) on an `aria-hidden` mirror behind it, a hover card for the token under the
 * pointer or the caret, and a list of variable names while a `{{` is being typed (an
 * ARIA combobox while the list is open). The input keeps its value, selection and events.
 */
export function VarInput({ tabKey, wrapperClassName, ...props }: Props) {
  const vars = useVars((s) => s.byTab[tabKey]);
  const collectionId = useTabs((s) => s.tabs.find((t) => t.key === tabKey)?.collectionId);
  const env = useWorkspace((s) => (collectionId ? (s.activeEnv[collectionId] ?? null) : null));
  const input = useRef<HTMLInputElement>(null);
  const mirror = useRef<HTMLDivElement>(null);
  const caretTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const leaveTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const pendingCaret = useRef<number | null>(null);
  const { note, say } = useCopy();
  const [shown, setShown] = useState<{ token: VarToken; anchor: Anchor } | null>(null);
  const [suggest, setSuggest] = useState<{ start: number; items: VarSuggestion[]; active: number } | null>(null);
  const tooltipId = useId();
  const listId = useId();
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
    // After a picked suggestion re-renders the value, put the caret after its `}}`.
    if (pendingCaret.current !== null) {
      el.setSelectionRange(pendingCaret.current, pendingCaret.current);
      pendingCaret.current = null;
    }
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
  const spanBox = (start: number): Anchor => {
    const span = mirror.current?.querySelector<HTMLElement>(`[data-start="${start}"]`);
    const r = span?.getBoundingClientRect() ?? input.current?.getBoundingClientRect();
    return { left: r?.left ?? 0, bottom: r?.bottom ?? 0 };
  };

  /**
   * Narrows or closes the name list for the caret position; it opens only while typing
   * (`mayOpen`), not when the caret is merely moved into a `{{token}}`.
   */
  const updateSuggest = (el: HTMLInputElement, mayOpen: boolean) => {
    if (!suggest && !mayOpen) return;
    const caret = el.selectionStart;
    const open = caret !== null && caret === el.selectionEnd ? openToken(el.value, caret) : null;
    const items = open ? suggestions(vars, open.query, env) : [];
    if (!open || items.length === 0) {
      if (suggest) setSuggest(null);
      return;
    }
    setShown(null);
    setSuggest((s) => ({ start: open.start, items, active: s && s.start === open.start ? Math.min(s.active, items.length - 1) : 0 }));
  };

  const pick = (item: VarSuggestion) => {
    const el = input.current;
    if (!el || el.selectionStart === null) return;
    const next = acceptSuggestion(el.value, el.selectionStart, item.name);
    pendingCaret.current = next.caret;
    setSuggest(null);
    typeValue(el, next.text);
    el.setSelectionRange(next.caret, next.caret);
  };

  const onCaret = () => {
    clearTimeout(caretTimer.current);
    const el = input.current;
    if (suggest || !el || el.selectionStart !== el.selectionEnd || el.selectionStart === null) return;
    const token = tokenAt(el.selectionStart);
    if (!token) {
      if (shown) setShown(null);
      return;
    }
    caretTimer.current = setTimeout(() => setShown({ token, anchor: spanBox(token.start) }), CARET_DELAY);
  };

  const onMouseMove = (e: MouseEvent<HTMLInputElement>) => {
    props.onMouseMove?.(e);
    clearTimeout(leaveTimer.current);
    if (suggest) return;
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
    if (suggest) {
      const last = suggest.items.length - 1;
      const move = (active: number) => {
        e.preventDefault();
        setSuggest({ ...suggest, active });
      };
      if (e.key === "ArrowDown") return move(suggest.active >= last ? 0 : suggest.active + 1);
      if (e.key === "ArrowUp") return move(suggest.active <= 0 ? last : suggest.active - 1);
      if ((e.key === "Enter" || e.key === "Tab") && !e.shiftKey && !e.metaKey && !e.ctrlKey && !e.altKey) {
        e.preventDefault();
        pick(suggest.items[suggest.active]!);
        return;
      }
      if (e.key === "Escape") {
        // Closes the list only: Esc must not also cancel a running send.
        e.preventDefault();
        setSuggest(null);
        return;
      }
    }
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
  // The mirror, with an empty marker where an open `{{` starts (the list sits under it).
  const anchorAt = suggest && !tokens.some((t) => t.start === suggest.start) ? suggest.start : null;
  const plain = (from: number, to: number): ReactNode[] => {
    if (anchorAt === null || anchorAt < from || anchorAt >= to) return [text.slice(from, to)];
    return [text.slice(from, anchorAt), <span key="anchor" data-start={anchorAt} />, text.slice(anchorAt, to)];
  };
  const pieces: ReactNode[] = [];
  let at = 0;
  for (const t of tokens) {
    pieces.push(...plain(at, t.start));
    const cls = vars ? explain(t.name, vars)?.kind : undefined;
    pieces.push(
      <span key={t.start} data-start={t.start} className={cls ? `var var-${cls.toLowerCase()}` : "var"}>
        {text.slice(t.start, t.end)}
      </span>,
    );
    at = t.end;
  }
  pieces.push(...plain(at, text.length));
  const optionId = (i: number) => `${listId}-${i}`;
  const combobox = suggest
    ? {
        role: "combobox",
        "aria-expanded": true,
        "aria-controls": listId,
        "aria-autocomplete": "list" as const,
        "aria-activedescendant": optionId(suggest.active),
      }
    : {};

  return (
    <span className={`var-input ${wrapperClassName ?? ""}`.trim()}>
      <div ref={mirror} className="var-mirror" aria-hidden="true">
        {pieces}
      </div>
      <input
        {...props}
        {...combobox}
        ref={input}
        aria-describedby={info ? tooltipId : props["aria-describedby"]}
        onChange={(e) => {
          props.onChange?.(e);
          updateSuggest(e.currentTarget, true);
        }}
        onKeyDown={onKeyDown}
        onKeyUp={(e) => {
          props.onKeyUp?.(e);
          if (!["ArrowDown", "ArrowUp", "Enter", "Tab", "Escape"].includes(e.key)) {
            updateSuggest(e.currentTarget, e.key.length === 1 && !e.metaKey && !e.ctrlKey);
          }
          onCaret();
        }}
        onSelect={(e) => {
          props.onSelect?.(e);
          onCaret();
        }}
        onClick={(e) => {
          props.onClick?.(e);
          updateSuggest(e.currentTarget, false);
          onCaret();
        }}
        onBlur={(e) => {
          props.onBlur?.(e);
          clearTimeout(caretTimer.current);
          setShown(null);
          setSuggest(null);
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
      {suggest && (
        <VarSuggest
          id={listId}
          optionId={optionId}
          items={suggest.items}
          active={suggest.active}
          onPick={pick}
          anchor={() => spanBox(suggest.start)}
        />
      )}
      {!suggest && shown && info && (
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
