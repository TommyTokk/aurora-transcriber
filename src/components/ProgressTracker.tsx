import type { TranscriptionSnapshot } from "../lib/transcription";
interface Props {
  job: TranscriptionSnapshot;
}
/** Announces processing phases and measurable chunk progress without fabricated estimates. */
export function ProgressTracker({ job }: Props) {
  const labels: Record<string, string> = {
    loading: "Preparing local models…",
    decoding: "Decoding audio…",
    vad: "Detecting speech…",
    transcribing: `Transcribing chunk ${job.currentChunk} of ${job.totalChunks}`,
    completed: job.segments.length
      ? "Transcription complete."
      : "No speech detected.",
    cancelled: "Transcription cancelled. Partial results were discarded.",
    error: "Transcription failed.",
  };
  if (!labels[job.phase]) return null;
  return (
    <section className="transcription-panel" aria-label="Processing progress">
      <p role="status" aria-live="polite">
        {labels[job.phase]}
      </p>
      {job.percentage !== null && (
        <progress
          max={100}
          value={job.percentage}
          aria-label="Transcription progress"
        />
      )}
    </section>
  );
}
