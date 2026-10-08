export interface Song {
  /** A track a tool made from another: which one, by which tool, how. */
  derived?: { from: string; fromTitle: string; tool: string; settings?: Record<string, unknown> } | null;
  id: string;
  title: string;
  lyrics: string;
  style: string;
  coverUrl: string;
  /** The page a cover photograph came from and the terms it is under. */
  coverSource?: string;
  /** Set while the cover is the look's placeholder, not one of the track's own. */
  coverPlaceholder?: string;
  duration: string;
  createdAt: Date;
  isGenerating?: boolean;
  /** Why a generation ended without a song; the card stays until it is removed. */
  failure?: string;
  /** The engine job a generation's row follows. */
  jobId?: string;
  /** The playlist the song being made goes into. */
  playlistId?: string;
  /** The job that made a library song; its row becomes this song. */
  madeByJob?: string;
  /** The list row this item is drawn in, when it took over another item's row. */
  viewKey?: string;
  queuePosition?: number; // Position in queue (undefined = actively generating, number = waiting in queue)
  progress?: number;
  stage?: string;
  /** The running stage's counter, e.g. `57/64 · ~2:50`. */
  stageDetail?: string;
  generationParams?: any;
  tags: string[];
  audioUrl?: string;
  /** The thumbs-up, kept with the song in the library. */
  liked?: boolean;
  /** The person's own note on the song, kept in the library. */
  note?: string;
  /** When it was liked: the liked list is read from the latest. */
  likedAt?: Date;
  isPublic?: boolean;
  likeCount?: number;
  viewCount?: number;
  userId?: string;
  creator?: string;
  creator_avatar?: string;
  ditModel?: string;
  lmModel?: string;
  lmBackend?: string;
  openrouterModel?: string | null;
  generationTime?: number;
  lrcContent?: string;
  bpm?: number;
  keyScale?: string;
  timeSignature?: string;
  /** The track carries its semantic stream, so POST /v1/music/replay can re-render it. */
  nativeReplayAvailable?: boolean;
  /** Processed versions kept beside the original; the active one plays. */
  audioVersions?: SongVersion[];
  /** `original`, a version id, or absent for a track never processed. */
  activeVersion?: string;
}

export interface SongVersion {
  id: string;
  label: string;
  createdAt: string;
  settings?: Record<string, unknown>;
}

export interface Playlist {
  id: string;
  name: string;
  description?: string;
  coverUrl?: string;
  cover_url?: string;
  songIds?: string[];
  isPublic?: boolean;
  is_public?: boolean;
  user_id?: string;
  creator?: string;
  created_at?: string;
  song_count?: number;
  songs?: any[];
}

export interface Comment {
  id: string;
  songId: string;
  userId: string;
  username: string;
  content: string;
  createdAt: Date;
}

/** One autoregressive stage's sampling preset; an absent knob is the checkpoint value. */
export interface YueSampling {
  temperature?: number;
  top_p?: number;
  top_k?: number;
  repetition_penalty?: number;
  penalty_window?: number;
  min_tokens?: number;
  max_tokens?: number;
}

export type YueCot = 'full' | 'melody' | 'off';
export type YueOutputFormat = 'flac' | 'mp3';

/**
 * A YuE2 request as `/v1/music/jobs` accepts it. Field names are the engine's
 * own (`Yue2Request` in yue2.cpp); anything left out is the engine default.
 */
export interface YueRequest {
  /** Style tags or a short style prompt, verbatim under `[Tags]`. */
  style: string;
  /** Lyrics with section tags, verbatim under `[Lyrics]`. */
  lyrics: string;
  /** ABC score to realise; empty lets the model write one. */
  abc?: string;
  /** Move a supplied score before singing, in semitones. */
  transpose?: number;
  /** With a supplied score, each section's words wait until the score reaches it; on unless false. */
  lyric_timing?: boolean;
  vocals_only?: boolean;
  cot?: YueCot;
  /** Target length; the model may end the song earlier. */
  duration_seconds?: number;
  /** Seed of the token draw: score, melody and length. */
  lm_seed?: number;
  /** Seed of the acoustic noise. */
  seed?: number;
  steps?: number;
  lm_batch_size?: number;
  synth_batch_size?: number;
  cfg_scale?: number;
  /** Strength of the realaudio decoder companion; 0 decodes with the checkpoint alone. */
  companion_scale?: number;
  semantic_tokens?: string;
  abc_sampling?: YueSampling;
  semantic_sampling?: YueSampling;
  output_format?: YueOutputFormat;
  mp3_bitrate?: number;
  /** Library title only, never sent to the engine. */
  title?: string;
  /** The library song whose melody this cover sings; the new song names it. */
  cover_of?: string;
  cover_prompt?: string;
  /** LoRA adapters for this song, each with a strength per engine slot. */
  adapters?: { id: string; scales: Record<string, number> }[];
  /** The playlist the made songs are added to: a project being worked on. */
  playlist_id?: string;
}

/** A song as the library stores and lists it. */
export interface NativeLibrarySong {
  id: string;
  title: string;
  audio_path?: string | null;
  caption: string;
  lyrics: string;
  metadata?: Record<string, unknown> | null;
  generation_settings?: Record<string, unknown> | null;
  engine_id: string;
  profile_id?: string | null;
  replay_request?: unknown | null;
  audio_codes?: unknown | null;
  created_at: string;
  updated_at?: string;
}

export interface YueJobSong {
  id: string;
  audio_url: string;
  /** The library's record of the song, as the library lists it. */
  song: NativeLibrarySong;
}

export interface YueJob {
  id: string;
  /** The mark this window gave the request; an agent's job has none. */
  client_ref?: string;
  /** When the service took the request, in Unix milliseconds. */
  submitted_at: number;
  status: 'queued' | 'running' | 'completed' | 'failed' | 'cancelled';
  phase: string;
  message: string;
  title?: string;
  style: string;
  lyrics: string;
  duration_seconds: number;
  generation_settings: Record<string, unknown>;
  playlist_id?: string;
  song?: YueJobSong;
  songs?: YueJobSong[];
  /** How the lyrics were laid along a score that came without sections. */
  laid?: { seconds: number; ceiling: number; crowded: { ratio: number; notes: number; syllables: number } | null };
}

/** What the engine log says the running job is doing. */
export interface YueProgress {
  stage: 'score' | 'semantic' | 'acoustic' | 'decode' | 'transcribe';
  fraction: number;
  detail: string;
}


export interface PlayerState {
  currentSong: Song | null;
  isPlaying: boolean;
  progress: number;
  volume: number;
}

export interface User {
  id: string;
  username: string;
  createdAt: Date;
  followerCount?: number;
  followingCount?: number;
  isFollowing?: boolean;
  isAdmin?: boolean;
  avatar_url?: string;
  banner_url?: string;
}

export interface UserProfile {
  user: User;
  publicSongs: Song[];
  publicPlaylists: Playlist[];
  stats: {
    totalSongs: number;
    totalLikes: number;
  };
}

// Simplified views for ACE-Step UI
export type View = 'create' | 'library' | 'tools' | 'adapters' | 'playlist' | 'search' | 'news';
