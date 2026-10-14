import assert from "node:assert/strict";
import test from "node:test";
import { LANGUAGE_OPTIONS, isTranscriptionLanguage } from "./settings.ts";

/** Ensures the requested labels map to the supported IPC values and reject unknown languages. */
test("language choices expose exactly the supported source hints", () => {
  assert.deepEqual(LANGUAGE_OPTIONS, [
    { value: "auto", label: "Auto-detect" },
    { value: "it", label: "Italiano" },
    { value: "en", label: "English" },
  ]);
  for (const option of LANGUAGE_OPTIONS)
    assert.equal(isTranscriptionLanguage(option.value), true);
  for (const value of ["", "fr", "Italiano", "English", "AUTO"])
    assert.equal(isTranscriptionLanguage(value), false);
});
