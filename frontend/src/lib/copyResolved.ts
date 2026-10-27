import { api } from "../api/instance";
import { useTabs } from "../store/tabs";
import { useWorkspace } from "../store/workspace";

export const SECRET_COPY_CONFIRM = "This value is a secret. Copy anyway?";

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/**
 * Copies `text` of request tab `tabKey` with its `{{variables}}` filled in for the
 * selected environment. A result holding a secret is fetched only after the user
 * confirms. Resolves to the live note: `label` when copied, `Not copied`, or the error.
 */
export async function copyResolved(tabKey: string, text: string, label: string): Promise<string> {
  const tab = useTabs.getState().tabs.find((t) => t.key === tabKey);
  if (!tab) return "Not copied";
  const env = useWorkspace.getState().activeEnv[tab.collectionId] ?? null;
  const body = { item: tab.item, draft: tab.draft, env, text, reveal: false };
  try {
    let resolved = await api().resolveText(tab.collectionId, body);
    if (resolved.text === null) {
      if (!window.confirm(SECRET_COPY_CONFIRM)) return "Not copied";
      resolved = await api().resolveText(tab.collectionId, { ...body, reveal: true });
    }
    await navigator.clipboard.writeText(resolved.text ?? "");
    return label;
  } catch (e) {
    return `Could not copy: ${message(e)}`;
  }
}
