import assert from "node:assert/strict";
import test from "node:test";
import { EMPTY_JOB, isJobActive, isNewSnapshot } from "./transcription.ts";

/** Ensures late events from an old job cannot replace a newer job or edited terminal result. */
test("snapshots reject duplicates and stale job events", () => {
  const current = { ...EMPTY_JOB, jobId: "new", revision: 12 };
  assert.equal(
    isNewSnapshot(current, { ...EMPTY_JOB, jobId: "old", revision: 11 }),
    false,
  );
  assert.equal(isNewSnapshot(current, current), false);
  assert.equal(isNewSnapshot(current, { ...current, revision: 13 }), true);
});
/** Verifies only processing phases keep controls locked. */
test("only active phases retain the background job", () => {
  for (const phase of ["loading", "decoding", "vad", "transcribing"])
    assert.equal(isJobActive(phase), true);
  for (const phase of ["idle", "completed", "cancelled", "error"])
    assert.equal(isJobActive(phase), false);
});
