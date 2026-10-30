import { jsonProblem } from "../lib/prettyJson";
import { rangeOf, type TextModel } from "./varDecorations";

export interface JsonMarker {
  startLineNumber: number;
  startColumn: number;
  endLineNumber: number;
  endColumn: number;
  message: string;
}

/**
 * The syntax error of a JSON body, if any. Monaco's own JSON check is off: it marks every
 * unquoted `{{variable}}`, which Bruno bodies are allowed to have.
 */
export function jsonMarkers(model: TextModel): JsonMarker[] {
  const problem = jsonProblem(model.getValue());
  return problem ? [{ ...rangeOf(model, problem.start, problem.end), message: problem.message }] : [];
}
