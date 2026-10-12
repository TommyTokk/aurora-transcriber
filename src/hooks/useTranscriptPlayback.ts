import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { EMPTY_PLAYBACK, ReviewPlayback } from "../lib/playback";

/** Owns a single clip; edits to transcript text do not recreate or interrupt it. */
export function useTranscriptPlayback(
  audioId: string | null,
  available: boolean,
) {
  const [snapshot, setSnapshot] = useState({ ...EMPTY_PLAYBACK });
  const controller = useRef<ReviewPlayback | null>(null);
  useEffect(() => {
    setSnapshot({ ...EMPTY_PLAYBACK });
    if (!audioId || !available) return;
    const review = new ReviewPlayback({
      media: new Audio(),
      load: (index) =>
        invoke<ArrayBuffer>("load_transcript_block", { audioId, index }),
      release: () => invoke("release_transcript_audio", { audioId }),
      createUrl: (bytes) =>
        URL.createObjectURL(new Blob([bytes], { type: "audio/wav" })),
      revokeUrl: (url) => URL.revokeObjectURL(url),
      changed: setSnapshot,
    });
    controller.current = review;
    return () => {
      controller.current = null;
      review.dispose();
    };
  }, [audioId, available]);
  return {
    snapshot,
    toggle: (index: number, duration: number) => {
      void controller.current?.toggle(index, duration);
    },
    seek: (position: number) => controller.current?.seek(position),
    setRate: (rate: number) => controller.current?.setRate(rate),
  };
}
