import { useEffect, useRef, useState } from "react";
import { api } from "../../api/instance";
import { useTabs } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";
import { MoreIcon } from "../icons";
import { Menu, type MenuItem } from "../Menu";

const NOTE_MS = 4000;

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/**
 * The URL bar's `⋯` menu: Copy as cURL (as written or with variables filled in), Copy
 * resolved URL, Save, and the request file (reveal it in the desktop app, copy its path
 * in a browser). Scratch tabs have no file, so the file item is left out.
 */
export function RequestMenu({ tabKey }: { tabKey: string }) {
  const [note, setNote] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const reveal = useWorkspace((s) => s.capabilities?.reveal ?? false);
  const tab = useTabs((s) => s.tabs.find((t) => t.key === tabKey));

  useEffect(() => () => clearTimeout(timer.current), []);

  const show = (text: string) => {
    setNote(text);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setNote(null), NOTE_MS);
  };

  const current = () => useTabs.getState().tabs.find((t) => t.key === tabKey);
  const envOf = (collectionId: string) => useWorkspace.getState().activeEnv[collectionId] ?? null;

  const copyCurl = async (resolve: boolean) => {
    const t = current();
    if (!t) return;
    try {
      const result = await api().copyCurl(t.collectionId, {
        item: t.item,
        draft: t.draft,
        env: envOf(t.collectionId),
        resolve,
      });
      if (result.containsSecret && !window.confirm("This command contains secret values. Copy anyway?")) return;
      await navigator.clipboard.writeText(result.text);
      show(result.omitted.length ? `Copied without ${result.omitted.join(", ")}` : "Copied");
    } catch (e) {
      show(`Could not copy: ${message(e)}`);
    }
  };

  const copyResolvedUrl = async () => {
    const t = current();
    if (!t) return;
    try {
      const result = await api().resolvedUrl(t.collectionId, {
        item: t.item,
        draft: t.draft,
        env: envOf(t.collectionId),
      });
      await navigator.clipboard.writeText(result.url);
      show(result.masked ? "Copied (secret values as ***)" : "Copied");
    } catch (e) {
      show(`Could not copy: ${message(e)}`);
    }
  };

  const onFile = () => {
    const t = current();
    if (!t || t.item === null) return;
    const item = t.item;
    if (reveal) {
      void api()
        .reveal(t.collectionId, item)
        .catch((e) => show(`Could not reveal: ${message(e)}`));
      return;
    }
    const root = useWorkspace.getState().collections.find((c) => c.id === t.collectionId)?.root;
    const path = root ? `${root.replace(/[\\/]+$/, "")}/${item}` : item;
    void navigator.clipboard
      .writeText(path)
      .then(() => show("Copied"))
      .catch((e) => show(`Could not copy: ${message(e)}`));
  };

  const items: MenuItem[] = [
    { label: "Copy as cURL", onSelect: () => void copyCurl(false) },
    { label: "Copy as cURL (variables filled in)", onSelect: () => void copyCurl(true) },
    { label: "Copy resolved URL", onSelect: () => void copyResolvedUrl() },
    { label: "Save", hint: "⌘S", onSelect: () => useTabs.getState().requestSave(tabKey) },
    ...(tab && tab.item !== null ? [{ label: reveal ? "Reveal .bru file" : "Copy file path", onSelect: onFile }] : []),
  ];

  return (
    <span className="request-menu">
      <Menu label="More request actions" items={items}>
        <MoreIcon />
      </Menu>
      <span className="copy-note" aria-live="polite">
        {note}
      </span>
    </span>
  );
}
