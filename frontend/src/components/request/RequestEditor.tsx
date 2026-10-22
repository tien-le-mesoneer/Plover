import { useState } from "react";
import type { HttpRequestDto } from "../../api/types";
import { switchBodyMode } from "../../lib/body";
import { urlWithQuery } from "../../lib/url";
import { useTabs, type Tab } from "../../store/tabs";
import { Banner } from "../Banner";
import { ResponseViewer } from "../ResponseViewer";
import { AuthTab } from "./AuthTab";
import { BodyTab } from "./BodyTab";
import { CodeEditor } from "../../editor/CodeEditor";
import { ScriptTab } from "./ScriptTab";
import { KeyValueTable } from "./KeyValueTable";
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
  const { edit, save, send, isDirty, resolveConflict } = useTabs();
  const onEdit = (f: (d: HttpRequestDto) => HttpRequestDto) => edit(tab.key, f);
  const draft = tab.draft;

  return (
    <div className="request-editor">
      <UrlBar
        draft={draft}
        dirty={isDirty(tab.key)}
        sending={tab.sending}
        onEdit={onEdit}
        onSend={() => void send(tab.key)}
        onSave={() => void save(tab.key)}
      />
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
          <nav className="subtabs">
            {SECTIONS.map((s) => {
              const n = count(draft, s);
              return (
                <button key={s} className={s === section ? "active" : ""} onClick={() => setSection(s)}>
                  {s}
                  {n ? <sup>{n}</sup> : hasCode(draft, s) ? <sup>•</sup> : null}
                </button>
              );
            })}
          </nav>
          <div className="pane-body">
            {section === "Params" && (
              <>
                <h4>Query</h4>
                <KeyValueTable
                  rows={draft.query}
                  onChange={(query) => onEdit((d) => ({ ...d, query, url: urlWithQuery(d.url, query) }))}
                />
                {draft.pathParams.length > 0 && (
                  <>
                    <h4>Path</h4>
                    <KeyValueTable
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
                rows={draft.headers}
                onChange={(headers) => onEdit((d) => ({ ...d, headers }))}
              />
            )}
            {section === "Auth" && <AuthTab auth={draft.auth} onChange={(auth) => onEdit((d) => ({ ...d, auth }))} />}
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
                namePlaceholder="res.status"
                valuePlaceholder="eq 200"
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
          <ResponseViewer result={tab.result} sending={tab.sending} />
        </section>
      </div>
    </div>
  );
}
