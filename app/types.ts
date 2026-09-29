export interface Song {
  /** A track a tool made from another: which one, by which tool, how. */
  derived?: { from: string; fromTitle: string; tool: string; settings?: Record<string, unknown> } | null;
  id: string;
  title: string;
  lyrics: string;
  style: string;
  coverUrl: string;
  duration: string;
  createdAt: Date;
  /** When it was last touched: renamed, liked, re-covered. Falls back to createdAt. */
  updatedAt?: Date;
  /** When the person last gave it a thumbs-up: only a like moves this, nothing else. */
  likedAt?: Date;
  isGenerating?: boolean;
  jobId?: string; // Active generation job ID for cancel
  queuePosition?: number; // Position in queue (undefined = actively generating, number = waiting in queue)
  progress?: number;
  stage?: string;
  generationParams?: any;
  tags: string[];
  audioUrl?: string;
  /** The user's thumbs-up: it lives with the song in the studio's library. */
  liked?: boolean;
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
export type YueOutputFormat = 'mp3' | 'wav16' | 'wav24' | 'wav32';

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
  semantic_tokens?: string;
  abc_sampling?: YueSampling;
  semantic_sampling?: YueSampling;
  peak_clip?: number;
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

export interface YueJobSong {
  id: string;
  audio_url: string;
}

export interface YueJob {
  id: string;
  /** The mark this window gave the request; an agent's job has none. */
  client_ref?: string;
  status: 'queued' | 'running' | 'completed' | 'failed' | 'cancelled';
  phase: string;
  message: string;
  title?: string;
  style: string;
  lyrics: string;
  duration_seconds: number;
  generation_settings: Record<string, unknown>;
  song?: YueJobSong;
  songs?: YueJobSong[];
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
