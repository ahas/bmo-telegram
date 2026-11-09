// The provider's policy as the reader reads it: the same file the service reads
// (`policy/telegram.toml`), so a placeholder or a name means the same on either side of the seam.

import policy from "../../policy/telegram.toml";

export interface ReaderPolicy {
  pictures: { mark: string };
  gathering: {
    file_not_kept: string;
    voice_name: string;
    sound_name: string;
    video_name: string;
    video_note_name: string;
    gif_name: string;
    video_fallback_extension: string;
    audio_extensions: Record<string, string>;
    video_extensions: Record<string, string>;
  };
  kinds: {
    photo: string;
    sticker: string;
    video: string;
    video_note: string;
    gif: string;
    voice: string;
    audio: string;
    document: string;
    unnamed_file: string;
  };
}

export const POLICY = policy as unknown as ReaderPolicy;

/** A line with each `{name}` mark filled by its value; a mark with no value stands as it is written. */
export function fill(line: string, values: Record<string, string>): string {
  return line.replaceAll(/\{([A-Za-z0-9_]+)\}/g, (mark, name: string) => values[name] ?? mark);
}
