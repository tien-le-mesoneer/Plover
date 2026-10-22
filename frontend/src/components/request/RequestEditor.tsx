import { useState } from "react";
import type { HttpRequestDto } from "../../api/types";
import { switchBodyMode } from "../../lib/body";
import { urlWithQuery } from "../../lib/url";
import { useTabs, type Tab } from "../../store/tabs";
import { ResponseViewer } from "../ResponseViewer";
import { AuthTab } from "./AuthTab";
import { BodyTab } from "./BodyTab";
import { KeyValueTable } from "./KeyValueTable";
import { UrlBar } from "./UrlBar";

const SECTIONS = ["Params", "Headers", "Auth", "Body"] as const;
type Section = (typeof SECTIONS)[number];

function count(draft: HttpRequestDto, section: Section): number | null {
  switch (section) {
    case "Params":
      return draft.query.filter((q) => q.enabled).length + draft.pathParams.length;
    case "Headers":
      return draft.headers.filter((h) => h.enabled).length;
    default:
      return null;
  }
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
        <div className="banner warn" role="alert">
          This file changed on disk since you opened it.
          <button onClick={() => void resolveConflict(tab.key, "reload")}>Reload from disk</button>
          <button onClick={() => void resolveConflict(tab.key, "overwrite")}>Overwrite with my changes</button>
        </div>
      )}
      {tab.error && (
        <div className="banner error" role="alert">
          {tab.error}
        </div>
      )}
      <div className="split">
        <section className="pane">
          <nav className="subtabs">
            {SECTIONS.map((s) => {
              const n = count(draft, s);
              return (
                <button key={s} className={s === section ? "active" : ""} onClick={() => setSection(s)}>
                  {s}
                  {n ? <sup>{n}</sup> : null}
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
          <ResponseViewer response={tab.response} sending={tab.sending} />
        </section>
      </div>
    </div>
  );
}
