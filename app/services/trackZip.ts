import { apiUrl } from './apiBase';
import { saveFile } from './saveFile';

export async function saveTrackZip(songIds: string[]): Promise<void> {
  const response = await fetch(apiUrl('/v1/exports/tracks'), {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ song_ids: songIds }),
  });
  const archive = await response.json();
  if (!response.ok) throw new Error(archive.error || `HTTP ${response.status}`);
  await saveFile(archive.file_name, { url: archive.url });
}
