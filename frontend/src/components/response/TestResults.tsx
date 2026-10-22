import type { AssertResultDto, TestResultDto } from "../../api/types";

/** Asserts and tests count together, like Bruno's response "Tests" tab. */
export function testSummary(tests: TestResultDto[], asserts: AssertResultDto[]): { passed: number; total: number } {
  const all = [...asserts.map((a) => a.passed), ...tests.map((t) => t.passed)];
  return { passed: all.filter(Boolean).length, total: all.length };
}

export function TestResults({ tests, asserts }: { tests: TestResultDto[]; asserts: AssertResultDto[] }) {
  const { passed, total } = testSummary(tests, asserts);
  if (total === 0) return <p className="muted center">No tests or asserts ran for this request.</p>;
  return (
    <div className="test-results">
      <p className={passed === total ? "ok" : "err"}>
        {passed}/{total} passed
      </p>
      <ul>
        {asserts.map((a, i) => (
          <li key={`a${i}`} className={a.passed ? "pass" : "fail"}>
            <span className="mark">{a.passed ? "✓" : "✗"}</span>
            <span className="muted">assert</span> <code>{`${a.lhs}: ${a.rhs}`}</code>
            {a.error && <div className="error">{a.error}</div>}
          </li>
        ))}
        {tests.map((t, i) => (
          <li key={`t${i}`} className={t.passed ? "pass" : "fail"}>
            <span className="mark">{t.passed ? "✓" : "✗"}</span>
            <span>{t.name}</span>
            {t.error && <div className="error">{t.error}</div>}
          </li>
        ))}
      </ul>
    </div>
  );
}
