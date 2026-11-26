import { apiUrl } from './apiBase';

const EXTENSIONS: Record<string, string> = { 'audio/wav': 'wav', 'audio/x-wav': 'wav', 'audio/wave': 'wav', 'audio/mpeg': 'mp3', 'audio/flac': 'flac', 'audio/ogg': 'ogg', 'audio/mp4': 'm4a' };

/** A library track's sound as a file, for what takes files: the cloud recogniser, a new training dataset. */
export async function libraryTrackFile(songId: string, title: string): Promise<File> {
  const response = await fetch(apiUrl(`/v1/library/media/${encodeURIComponent(songId)}`));
  if (!response.ok) throw new Error(`${title}: ${response.status}`);
  const blob = await response.blob();
  return new File([blob], `${title}.${EXTENSIONS[blob.type] ?? 'wav'}`, { type: blob.type || 'audio/wav' });
}
