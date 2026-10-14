import assert from "node:assert/strict";
import test from "node:test";
import {
  effectiveFileLanguage,
  synchronizeFileLanguage,
  type FileLanguageSelection,
} from "./fileLanguage.ts";

/** Untouched imports follow updated saved defaults, while explicit choices remain independent. */
test("file language follows settings until explicitly overridden", () => {
  const initial = synchronizeFileLanguage(
    { audioId: null, language: null },
    "first-import",
  );
  assert.equal(effectiveFileLanguage(initial, "it"), "it");
  assert.equal(effectiveFileLanguage(initial, "en"), "en");
  const local: FileLanguageSelection = { ...initial, language: "auto" };
  assert.equal(effectiveFileLanguage(local, "it"), "auto");
  assert.equal(effectiveFileLanguage(local, "en"), "auto");
  const restored: FileLanguageSelection = { ...local, language: null };
  assert.equal(effectiveFileLanguage(restored, "it"), "it");
  assert.equal(effectiveFileLanguage(restored, "en"), "en");
});

/** Navigation, retries, and a cancelled replacement picker retain the same native identity. */
test("same imported file preserves its language override", () => {
  const selection: FileLanguageSelection = {
    audioId: "first-import",
    language: "en",
  };
  assert.equal(synchronizeFileLanguage(selection, "first-import"), selection);
  assert.equal(
    effectiveFileLanguage(
      synchronizeFileLanguage(selection, "first-import"),
      "it",
    ),
    "en",
  );
});

/** Reimporting even the same path gives a new identity and removal permanently discards the override. */
test("new imports and removal reset the local choice", () => {
  const selection: FileLanguageSelection = {
    audioId: "first-import",
    language: "en",
  };
  const replacement = synchronizeFileLanguage(selection, "second-import");
  assert.deepEqual(replacement, { audioId: "second-import", language: null });
  assert.equal(effectiveFileLanguage(replacement, "it"), "it");
  const removed = synchronizeFileLanguage(selection, null);
  assert.deepEqual(removed, { audioId: null, language: null });
  assert.deepEqual(synchronizeFileLanguage(removed, "first-import"), {
    audioId: "first-import",
    language: null,
  });
});
