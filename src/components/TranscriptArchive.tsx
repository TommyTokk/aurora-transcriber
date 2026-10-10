import type { TranscriptSummary } from "../lib/transcription";
interface Props {
  entries: TranscriptSummary[];
  disabled: boolean;
  onOpen: (id: string) => void;
}
/** Lists explicitly saved documents without revealing source audio paths. */
export function TranscriptArchive({ entries, disabled, onOpen }: Props) {
  return (
    <section className="transcription-panel" aria-label="Saved transcripts">
      <h2>Saved transcripts</h2>
      {entries.length === 0 ? (
        <p className="muted">Your saved transcripts will appear here.</p>
      ) : (
        <ul className="archive-list">
          {entries.map((entry) => (
            <li key={entry.id}>
              <button
                className="archive-entry"
                disabled={disabled}
                onClick={() => onOpen(entry.id)}
              >
                <span>{entry.name}</span>
                <time dateTime={new Date(entry.updatedAt * 1000).toISOString()}>
                  {new Date(entry.updatedAt * 1000).toLocaleString()}
                </time>
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
