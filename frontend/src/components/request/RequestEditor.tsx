import { useState } from "react";
import { api } from "../../api/instance";
import type { CurlNoticeDto, HttpRequestDto } from "../../api/types";
import { switchBodyMode } from "../../lib/body";
import { urlWithQuery } from "../../lib/url";
import { useTabs, type Tab } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";
import { RequestMenu } from "./RequestMenu";
import { CurlBanner, secretMoves } from "./CurlBanner";
import { Banner } from "../Banner";
import { ResponseViewer } from "../ResponseViewer";
import { AuthTab } from "./AuthTab";
import { BodyTab } from "./BodyTab";
import { CodeEditor } from "../../editor/CodeEditor";
import { ScriptTab } from "./ScriptTab";
import { KeyValueTable } from "./KeyValueTable";
import { SubTabs } from "../SubTabs";
import { UrlBar } from "./UrlBar";

const SECTIONS = ["Params", "Headers", "Auth", "Body", "Script", "Assert", "Tests"] as const;
type Section = (typeof SECTIONS)[number];

function count(draft: HttpRequestDto, section: Section): number | null {
  switch (section) {
    case "Params":
      return draft.query.filter((q) => q.enabled).length + draft.pathParams.length;
    case "Headers":
      return draft.headers.filter((h) => h.enabled).length;
    case "Assert":
      return draft.asserts.filter((a) => a.enabled).length;
    default:
      return null;
  }
}

function hasCode(draft: HttpRequestDto, section: Section): boolean {
  if (section === "Script") return Boolean(draft.scriptPre || draft.scriptPost);
  if (section === "Tests") return Boolean(draft.tests);
  return false;
}

export function RequestEditor({ tab }: { tab: Tab }) {
  const [section, setSection] = useState<Section>(tab.draft.body.mode === "none" ? "Params" : "Body");
  const { edit, send, cancel, resolveConflict } = useTabs();
  const onEdit = (f: (d: HttpRequestDto) => HttpRequestDto) => edit(tab.key, f);
  const draft = tab.draft;
  const envName = useWorkspace((s) => s.activeEnv[tab.collectionId] ?? null);
  const env = useWorkspace((s) => s.environments[tab.collectionId]?.find((e) => e.name === envName));
  // The last curl paste: what it could not import, and the draft before it (for Undo).
  const [pasted, setPasted] = useState<{ notices: CurlNoticeDto[]; previous: HttpRequestDto } | null>(null);
  const [pasteError, setPasteError] = useState<string | null>(null);
  const [moving, setMoving] = useState(false);
  const moves = pasted ? secretMoves(draft, env) : [];

  const onCurl = async (text: string) => {
    setPasteError(null);
    try {
      const parsed = await api().parseCurl(text);
      const previous = useTabs.getState().applyCurl(tab.key, parsed.request);
      setPasted({ notices: parsed.notices, previous });
    } catch (e) {
      setPasted(null);
      setPasteError(`Not a curl command Plover can read: ${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const moveToSecret = async () => {
    if (!envName || moves.length === 0) return;
    const values: Record<string, string> = {};
    for (const m of moves) values[m.variable] = draft.headers[m.index]!.value;
    setMoving(true);
    try {
      await useWorkspace.getState().setSecrets(tab.collectionId, envName, values, true);
      onEdit((d) => ({
        ...d,
        headers: d.headers.map((h, i) => {
          const move = moves.find((m) => m.index === i);
          return move ? { ...h, value: `{{${move.variable}}}` } : h;
        }),
      }));
    } catch (e) {
      setPasteError(`Could not store the secret: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setMoving(false);
    }
  };

  return (
    <div className="request-editor">
      <UrlBar
        draft={draft}
        sending={tab.sending}
        sendStartedAt={tab.sendStartedAt}
        onEdit={onEdit}
        onSend={() => void send(tab.key)}
        onCancel={() => void cancel(tab.key)}
        onCurl={(text) => void onCurl(text)}
        actions={<RequestMenu tabKey={tab.key} />}
        tabKey={tab.key}
      />
      {pasted && (
        <CurlBanner
          notices={pasted.notices}
          canMove={moves.length > 0}
          env={envName}
          moving={moving}
          onUndo={() => {
            const previous = pasted.previous;
            onEdit(() => previous);
            setPasted(null);
          }}
          onMoveToSecret={() => void moveToSecret()}
          onDismiss={() => setPasted(null)}
        />
      )}
      {pasteError && (
        <Banner kind="error" role="alert">
          {pasteError}
        </Banner>
      )}
      {tab.conflict && (
        <Banner
          kind="warn"
          role="alert"
          actions={
            <>
              <button onClick={() => void resolveConflict(tab.key, "reload")}>Reload from disk</button>
              <button onClick={() => void resolveConflict(tab.key, "overwrite")}>Overwrite with my changes</button>
            </>
          }
        >
          This file changed on disk since you opened it.
        </Banner>
      )}
      {tab.error && (
        <Banner kind="error" role="alert">
          {tab.error}
        </Banner>
      )}
      <div className="split">
        <section className="pane">
          <SubTabs
            label="Request sections"
            value={section}
            onChange={setSection}
            tabs={SECTIONS.map((s) => {
              const n = count(draft, s);
              const code = hasCode(draft, s);
              return {
                id: s,
                label: (
                  <>
                    {s}
                    {n ? <sup aria-hidden="true">{n}</sup> : code ? <sup aria-hidden="true">•</sup> : null}
                  </>
                ),
                name: n ? `${s}, ${n}` : code ? `${s}, has code` : s,
              };
            })}
          />
          <div className="pane-body">
            {section === "Params" && (
              <>
                <h4>Query</h4>
                <KeyValueTable
                  varTabKey={tab.key}
                  rows={draft.query}
                  onChange={(query) => onEdit((d) => ({ ...d, query, url: urlWithQuery(d.url, query) }))}
                />
                {draft.pathParams.length > 0 && (
                  <>
                    <h4>Path</h4>
                    <KeyValueTable
                      varTabKey={tab.key}
                      rows={draft.pathParams}
                      fixedNames
                      onChange={(pathParams) => onEdit((d) => ({ ...d, pathParams }))}
                    />
                  </>
                )}
              </>
            )}
            {section === "Headers" && (
              <KeyValueTable
                varTabKey={tab.key}
                rows={draft.headers}
                onChange={(headers) => onEdit((d) => ({ ...d, headers }))}
              />
            )}
            {section === "Auth" && (
              <AuthTab tabKey={tab.key} auth={draft.auth} onChange={(auth) => onEdit((d) => ({ ...d, auth }))} />
            )}
            {section === "Script" && (
              <ScriptTab
                pre={draft.scriptPre}
                post={draft.scriptPost}
                onChange={(phase, script) =>
                  onEdit((d) => (phase === "pre" ? { ...d, scriptPre: script } : { ...d, scriptPost: script }))
                }
              />
            )}
            {section === "Assert" && (
              <KeyValueTable
                rows={draft.asserts}
                nameLabel="Assertion"
                valueLabel="Expected"
                hint="Example: res.status — eq 200, or res.body.name — isString"
                onChange={(asserts) => onEdit((d) => ({ ...d, asserts }))}
              />
            )}
            {section === "Tests" && (
              <div className="editor-box">
                <CodeEditor
                  value={draft.tests ?? ""}
                  language="javascript"
                  onChange={(v) => onEdit((d) => ({ ...d, tests: v === "" ? null : v }))}
                />
              </div>
            )}
            {section === "Body" && (
              <BodyTab
                body={draft.body}
                onChange={(body) => onEdit((d) => ({ ...d, body }))}
                onModeChange={(mode) => onEdit((d) => switchBodyMode(d, mode))}
              />
            )}
          </div>
        </section>
        <section className="pane">
          <ResponseViewer
            result={tab.result}
            sending={tab.sending}
            cancelledAfterMs={tab.cancelledAfterMs}
            name={draft.name}
            collectionId={tab.collectionId}
            onOpenScript={() => setSection("Script")}
          />
        </section>
      </div>
    </div>
  );
}
