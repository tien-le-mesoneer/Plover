import { useTabs } from "../store/tabs";

export function RequestTabs() {
  const { tabs, active, activate, close, isDirty } = useTabs();
  if (tabs.length === 0) return null;
  return (
    <div className="tabbar" role="tablist">
      {tabs.map((t) => {
        const dirty = isDirty(t.key);
        return (
          <div
            key={t.key}
            role="tab"
            aria-selected={t.key === active}
            className={`tab ${t.key === active ? "active" : ""}`}
            title={t.item}
            onClick={() => activate(t.key)}
            onAuxClick={(e) => e.button === 1 && close(t.key)}
          >
            <span className={`badge method-${t.draft.method.toLowerCase()}`}>{t.draft.method}</span>
            <span className="tab-name">{t.draft.name}</span>
            {dirty && <span className="dirty" title="Unsaved changes" />}
            <button
              className="icon"
              title="Close"
              onClick={(e) => {
                e.stopPropagation();
                if (!dirty || window.confirm(`Discard unsaved changes to "${t.draft.name}"?`)) close(t.key);
              }}
            >
              ×
            </button>
          </div>
        );
      })}
    </div>
  );
}
