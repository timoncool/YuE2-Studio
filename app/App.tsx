import React, { Activity, useState, useEffect, useRef, useCallback, useMemo } from 'react';
import type { TranslationKey } from './i18n/translations';
import { Sidebar } from './components/Sidebar';
import { CreatePanel, type CreateRequest } from './components/CreatePanel';
import { SongList } from './components/SongList';
import { TopControlPanel, type SessionView } from './components/TopControlPanel';
import {
  loadWorkspaces,
  activeWorkspace,
  adoptImportSession,
  sessionTitle,
  createWorkspace,
  updateWorkspace,
  openWorkspace,
  closeWorkspace,
  readPlaybackMode,
  writePlaybackMode,
  type Workspace,
  type RepeatMode,
  type SortOrder,
} from './services/workspaces';
import { SessionList, type WorkspaceSession } from './components/SessionList';
import { SessionCreateModal } from './components/SessionCreateModal';
import { SessionConfirmModal } from './components/SessionConfirmModal';
import { EditModeModal } from './components/EditModeModal';
import { AgentConfirmModal, type AgentPermissionDecision, type AgentPermissionRequest } from './components/AgentConfirmModal';
import { SessionChoiceModal, type SessionChoiceAnswer, type SessionChoiceRequest } from './components/SessionChoiceModal';
import { AgentJournalPanel, type AgentNotice } from './components/AgentJournalPanel';
import { RightSidebar } from './components/RightSidebar';
import { Player } from './components/Player';
import { LibraryView } from './components/LibraryView';
import { CreatePlaylistModal, AddToPlaylistModal } from './components/PlaylistModals';
import { CoverRegenModal } from './components/CoverRegenModal';
import { ReplayModal } from './components/ReplayModal';
import { ProcessingModal } from './components/ProcessingModal';
import { VideoGeneratorModal } from './components/VideoGeneratorModal';
import { SongActions, SongActionsProvider } from './context/SongActionsContext';
import { useBridgeCommand } from './services/mcpBridge';
import { apiUrl } from './services/apiBase';
import { SettingsModal } from './components/SettingsModal';
import { Song, YueRequest, YueJob, YueProgress, View, Playlist } from './types';
// Resizable panel hook
const PANEL_MAX_SHARE = 0.4;
const PANEL_KEY_STEP = 16;

/** A side panel the user sizes by dragging its edge, with the arrow keys once
 *  the edge has focus, or back to its default with a double click. Wide
 *  screens get wide panels; the middle always keeps the rest of the window. */
function useResizablePanel(key: string, defaultWidth: number, min: number, max: number, direction: 'left' | 'right' = 'left', label = '') {
  const limitNow = React.useCallback(() => Math.max(min, Math.min(max, Math.round(window.innerWidth * PANEL_MAX_SHARE))), [min, max]);
  const [width, setWidth] = React.useState(() => {
    const saved = Number(localStorage.getItem(`panel-${key}`));
    return Number.isFinite(saved) && saved > 0 ? saved : defaultWidth;
  });
  const [limit, setLimit] = React.useState(limitNow);
  React.useEffect(() => {
    const onResize = () => setLimit(limitNow());
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, [limitNow]);
  // a width saved on a larger window is drawn within this one, and kept for the larger one
  const shown = Math.min(Math.max(width, min), limit);

  const commit = React.useCallback((next: number) => {
    const clamped = Math.min(limitNow(), Math.max(min, Math.round(next)));
    setWidth(clamped);
    localStorage.setItem(`panel-${key}`, String(clamped));
  }, [key, min, limitNow]);

  const onMouseDown = React.useCallback((e: React.MouseEvent) => {
    const startX = e.clientX;
    const startW = shown;
    const bound = limitNow();
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
    const at = (ev: MouseEvent) => Math.min(bound, Math.max(min, startW + (direction === 'left' ? 1 : -1) * (ev.clientX - startX)));
    const onMouseMove = (ev: MouseEvent) => setWidth(at(ev));
    const onMouseUp = (ev: MouseEvent) => {
      commit(at(ev));
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      document.removeEventListener('mousemove', onMouseMove);
      document.removeEventListener('mouseup', onMouseUp);
    };
    document.addEventListener('mousemove', onMouseMove);
    document.addEventListener('mouseup', onMouseUp);
  }, [shown, min, direction, limitNow, commit]);

  const onKeyDown = React.useCallback((e: React.KeyboardEvent) => {
    const grow = direction === 'left' ? 'ArrowRight' : 'ArrowLeft';
    const shrink = direction === 'left' ? 'ArrowLeft' : 'ArrowRight';
    const step = e.shiftKey ? PANEL_KEY_STEP * 4 : PANEL_KEY_STEP;
    const next = e.key === grow ? shown + step : e.key === shrink ? shown - step : e.key === 'Home' ? min : e.key === 'End' ? limitNow() : null;
    if (next === null) return;
    e.preventDefault();
    commit(next);
  }, [shown, min, direction, limitNow, commit]);

  const handle = (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuenow={shown}
      aria-valuemin={min}
      aria-valuemax={limit}
      tabIndex={0}
      onMouseDown={onMouseDown}
      onDoubleClick={() => {
        // the default is kept as it is; a narrow window only draws it narrower
        setWidth(defaultWidth);
        localStorage.removeItem(`panel-${key}`);
      }}
      onKeyDown={onKeyDown}
      className="hidden md:flex w-[5px] shrink-0 items-center justify-center cursor-col-resize group z-20 relative bg-zinc-200/50 dark:bg-zinc-800 hover:bg-pink-500/30 focus-visible:bg-pink-500/40 focus-visible:outline-hidden transition-colors"
    >
      <div className="w-[3px] h-10 rounded-full bg-zinc-400/30 dark:bg-zinc-600/50 group-hover:bg-pink-500 group-focus-visible:bg-pink-500 transition-colors" />
    </div>
  );

  return { width: shown, handle };
}
import { getAudioUrl } from './services/api';
import { useAuth } from './context/AuthContext';
import { useResponsive } from './context/ResponsiveContext';
import { I18nProvider, useI18n } from './context/I18nContext';
import { splitByParent } from './components/songParts';
import { yue2 } from './i18n/yue2';
import { List } from 'lucide-react';
import { PlaylistDetail } from './components/PlaylistDetail';
import { Toast, ToastType } from './components/Toast';
import { FilesPanel } from './components/FilesPanel';
import { SearchPage } from './components/SearchPage';
import { NewsPage } from './components/NewsPage';
import { ConfirmDialog } from './components/ConfirmDialog';
import { SetupGate } from './components/SetupGate';
import { EngineStarting } from './components/EngineStarting';
import { StudioOffline } from './components/StudioOffline';
import { StudioToolsPanel } from './components/StudioToolsPanel';
import { AdaptersPage } from './components/AdaptersPage';
import { PlayerExtras } from './components/player/PlayerExtras';
import { audioGraph, registerPlayer, resumeAudioGraph } from './services/audioGraph';
import { serveVisualizerFeed } from './services/visualizerFeed';
import { winampControl, winampOn } from './services/winamp';
import { createNativePlaylist, deleteNativeSong, loadNativeLibrarySongs, loadNativePlaylists, setNativeSongLiked, updateNativePlaylist } from './services/nativeLibrary';

const NATIVE_LIKED_SONG_IDS_KEY = 'yue2-studio-liked-song-ids';

function loadNativeLikedSongIds(): Set<string> {
  try {
    const stored = JSON.parse(localStorage.getItem(NATIVE_LIKED_SONG_IDS_KEY) || '[]');
    return new Set(Array.isArray(stored) ? stored.filter((id): id is string => typeof id === 'string') : []);
  } catch {
    return new Set();
  }
}

/** The kind of thing a tool works on, said the way the log says it (mirrors journal_facts in the
 * service): the person reads "session: Night shift" and knows what is at stake, while the tool's own
 * name - "workspace_delete" - tells them nothing. */
const kindOf = (tool: string): TranslationKey => tool.includes('workspace') ? 'journalKindSession'
  : tool.includes('playlist') ? 'journalKindPlaylist'
    : tool.includes('stems') ? 'journalKindStems'
      : tool.includes('song') || tool.includes('library') ? 'journalKindSong'
        : 'journalKindOther';

/**
 * The words the log is written in. The service reports what was done and to what kind of
 * thing; a language only exists in the window, so the sentence is built here - never from
 * the tool's own name, which tells a person nothing about what was deleted or made.
 */
const JOURNAL_SAID: Record<string, TranslationKey> = {
  deleted: 'journalVerbDeleted',
  created: 'journalVerbCreated',
  updated: 'journalVerbUpdated',
  opened: 'journalVerbOpened',
  closed: 'journalVerbClosed',
  liked: 'journalVerbLiked',
  unliked: 'journalVerbUnliked',
  split: 'journalVerbSplit',
  imported: 'journalVerbImported',
  ran: 'journalVerbRan',
  session: 'journalKindSession',
  song: 'journalKindSong',
  playlist: 'journalKindPlaylist',
  stems: 'journalKindStems',
  other: 'journalKindOther',
};

/// "Rock (3)". A session name is unique, as a file name is: two sessions
/// that read the same would be painful to tell apart.
function uniqueSessionName(name: string, sessions: WorkspaceSession[]): string {
  const taken = new Set(sessions.map((session) => session.name));
  if (!taken.has(name)) return name;
  let index = 2;
  while (taken.has(`${name} (${index})`)) index += 1;
  return `${name} (${index})`;
}

function NativeUnavailableView({ title, detail }: { title: string; detail: string }): React.ReactElement {
  return (
    <div className="flex h-full min-h-0 flex-1 items-center justify-center overflow-y-auto bg-white px-6 py-10 dark:bg-suno">
      <section className="w-full max-w-xl rounded-2xl border border-amber-500/30 bg-amber-500/5 p-6 text-center shadow-xs">
        <p className="text-xs font-semibold uppercase tracking-[0.16em] text-amber-600 dark:text-amber-400">YuE2 Studio</p>
        <h1 className="mt-2 text-xl font-bold text-zinc-950 dark:text-white">{title}</h1>
        <p className="mt-3 text-sm leading-6 text-zinc-600 dark:text-zinc-300">{detail}</p>
      </section>
    </div>
  );
}


function AppContent() {
  // i18n
  const { t } = useI18n();

  // Responsive
  const { isMobile, isDesktop } = useResponsive();

  // Auth
  const { user } = useAuth();
  const leftPanel = useResizablePanel('create', 420, 320, 1200, 'left', t('createMusic'));
  const rightPanel = useResizablePanel('details', 400, 320, 1200, 'right', t('songDetails'));
  const [nativeSetupReady, setNativeSetupReady] = useState(false);
  // A track sent here from a menu's "separate into stems".
  const [stemsSongId, setStemsSongId] = useState<string | null>(null);
  // Which of the three situations the studio is in. It starts unknown, and
  // unknown must not look like "nothing is installed": showing the download
  // page for a second on every launch is how a ready studio was made to look
  // like a bill.
  const [nativeModels, setNativeModels] = useState<'unknown' | 'missing' | 'installed' | 'offline'>('unknown');
  useEffect(() => {
    const read = () => void fetch('/setup/status')
      .then((response) => (response.ok ? response.json() : Promise.reject(new Error())))
      .then((status: { ready?: boolean; engine_ready?: boolean }) => {
        setNativeModels(status.ready === true ? 'installed' : 'missing');
        if (status.ready && status.engine_ready) setNativeSetupReady(true);
      })
      // A service that does not answer is not an engine that is still coming
      // up: the application has been closed, and saying "starting" at a dead
      // process is the one thing the window must not do.
      .catch(() => {
        setNativeModels('offline');
        setNativeSetupReady(false);
      });
    read();
    const timer = window.setInterval(read, 2000);
    return () => window.clearInterval(timer);
  }, []);
  useEffect(() => {
    const open = (event: Event) => {
      setStemsSongId((event as CustomEvent<string>).detail);
      setCurrentView('tools');
    };
    const openSettings = (event: Event) => {
      setSettingsSection((event as CustomEvent<string>).detail);
      setShowSettingsModal(true);
    };
    // A page asked for by another page, the LoRA card sending the user to its library.
    const navigate = (event: Event) => {
      const view = (event as CustomEvent<View>).detail;
      if (view) setCurrentView(view);
    };
    const process = (event: Event) => {
      const song = (event as CustomEvent<Song>).detail;
      if (song) setSongToProcess(song);
    };
    window.addEventListener('yue:open-stems', open);
    window.addEventListener('yue:open-settings', openSettings);
    window.addEventListener('yue:navigate', navigate);
    window.addEventListener('yue:process-song', process);
    return () => {
      window.removeEventListener('yue:open-stems', open);
      window.removeEventListener('yue:open-settings', openSettings);
      window.removeEventListener('yue:navigate', navigate);
      window.removeEventListener('yue:process-song', process);
    };
  }, []);

  // Track multiple concurrent generation jobs
  const activeJobsRef = useRef<Map<string, { tempId: string; pollInterval: ReturnType<typeof setInterval> }>>(new Map());
  const [activeJobCount, setActiveJobCount] = useState(0);
  // Marks of the requests this window has sent and not yet tracked. The service
  // hands the mark back on the job, so the adopter below never takes a job of
  // this window's for an agent's while its response is still on the way.
  const ownRequestsRef = useRef<Set<string>>(new Set());

  // FIFO drain barrier — handlers awaiting it block until the active-jobs
  // queue is empty. Used by CreatePanel to chain LLM pre-flight calls behind
  // the previous track's full completion (LLM + audio + cover) — that's the
  // user's "queue" mental model: gen N+1 starts only after gen N is done.
  const queueDrainResolversRef = useRef<Array<() => void>>([]);
  const waitForJobsToDrain = useCallback((): Promise<void> => {
    if (activeJobsRef.current.size === 0) return Promise.resolve();
    return new Promise((resolve) => {
      queueDrainResolversRef.current.push(resolve);
    });
  }, []);
  const drainQueueWaiters = useCallback(() => {
    if (activeJobsRef.current.size !== 0) return;
    const waiters = queueDrainResolversRef.current;
    queueDrainResolversRef.current = [];
    waiters.forEach((r) => r());
  }, []);

  // "Pending click" counter — bumped synchronously the moment the user
  // clicks Создать, so the button shows N/10 instantly even before the LLM
  // pre-flight completes. Decremented when the click hands off to a real
  // active job (beginPollingJob has registered it in activeJobsRef).
  const [pendingClickCount, setPendingClickCount] = useState(0);
  const incrementPendingClicks = useCallback((n = 1) => setPendingClickCount(c => c + n), []);

  // Pre-flight AbortController registry, keyed by the placeholder card's
  // tempId. CreatePanel registers a controller right before it starts the
  // OpenRouter pre-flight call; the cancel buttons (single + cancel-all)
  // pull from here to actually abort the in-flight HTTP request, otherwise
  // the user's only escape is reloading the page (the Promise chain that
  // park clicks via `waitForJobsToDrain` doesn't have an abort path of
  // its own — see handoff "Open issue #1").
  const preflightAbortersRef = useRef<Map<string, AbortController>>(new Map());
  const registerPreflightAbort = useCallback((tempId: string, ac: AbortController) => {
    preflightAbortersRef.current.set(tempId, ac);
  }, []);
  const unregisterPreflightAbort = useCallback((tempId: string) => {
    preflightAbortersRef.current.delete(tempId);
  }, []);
  const decrementPendingClicks = useCallback((n = 1) => setPendingClickCount(c => Math.max(0, c - n)), []);

  // Instant temp-song factory — called from CreatePanel at click time so the
  // user sees a card in the list IMMEDIATELY, then it's promoted with real
  // data when LLM pre-flight + POST complete. Returns the tempId so the
  // caller can stash it on the eventual `onGenerate` payload (`_tempId`).
  const createTempSongForClick = useCallback((descriptionPreview: string): string => {
    const tempId = `temp_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
    const tempSong: Song = {
      id: tempId,
      title: descriptionPreview.slice(0, 60) || (t('generating') || 'Generating…'),
      lyrics: '',
      style: '',
      coverUrl: '',
      duration: '--:--',
      createdAt: new Date(),
      isGenerating: true,
      // Use the i18n key — SongList renders via t(song.stage) || song.stage.
      stage: 'stageWaitingInQueue',
      tags: ['queued'],
      isPublic: true,
    };
    setSongs(prev => [tempSong, ...prev]);
    return tempId;
  }, [t]);

  // Update placeholder fields as LLM streams data, e.g. style/lyrics.
  const updateTempSongForClick = useCallback((tempId: string, patch: Partial<Song>) => {
    setSongs(prev => prev.map(s => s.id === tempId ? { ...s, ...patch } : s));
  }, []);

  // Failure path — drop the placeholder so the user doesn't see a stuck "Queued…"
  // BUT only if the card is still a placeholder (no `jobId` yet). Once App.tsx
  // handleGenerate has POSTed and beginPollingJob set jobId on the song, the
  // card represents a real running backend job — wiping it would leave the
  // user with audio gen running invisibly. Skip in that case.
  const removeTempSongForClick = useCallback((tempId: string) => {
    setSongs(prev => prev.filter(s => {
      if (s.id !== tempId) return true;
      // Promoted to active job → keep
      if (s.jobId) return true;
      return false;
    }));
  }, []);

  // Theme State
  // Dark is the studio's own look, not a preference inherited from the desktop:
  // the interface was drawn for it, and a light Windows was turning a music
  // studio into a spreadsheet on first launch. A user who picks light keeps it.
  const [theme, setTheme] = useState<'dark' | 'light'>(() => {
    const stored = localStorage.getItem('theme');
    return stored === 'light' ? 'light' : 'dark';
  });

  // Navigation State - default to create view
  const [currentView, setCurrentView] = useState<View>('create');

  // Content State
  const [songs, setSongs] = useState<Song[]>([]);
  const [playlists, setPlaylists] = useState<Playlist[]>([]);
  // The thumbs-up lives with the song itself (metadata.liked), so the library
  // carries it and every window - and the agent - sees the same marks.
  const likedSongIds = useMemo(
    () => new Set(songs.filter((song) => song.liked).map((song) => song.id)),
    [songs],
  );
  const [playQueue, setPlayQueue] = useState<Song[]>([]);
  const [queueIndex, setQueueIndex] = useState(-1);

  // Selection State
  const [currentSong, setCurrentSong] = useState<Song | null>(null);
  const [selectedSong, setSelectedSong] = useState<Song | null>(null);
  const [selectedPlaylist, setSelectedPlaylist] = useState<Playlist | null>(null);

  // Player State
  const [isPlaying, setIsPlaying] = useState(false);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [volume, setVolume] = useState(() => {
    const stored = localStorage.getItem('volume');
    return stored ? parseFloat(stored) : 0.8;
  });
  const [playbackRate, setPlaybackRate] = useState(1.0);
  const [isShuffle, setIsShuffle] = useState(false);
  // stop: play this track to its end and stay there, as Winamp's and
  // foobar's "stop after current"; next and previous still move by hand
  const [repeatMode, setRepeatMode] = useState<'none' | 'all' | 'one' | 'stop'>('all');
  const repeatModeRef = useRef(repeatMode);
  repeatModeRef.current = repeatMode;

  // UI State
  const [isGenerating, setIsGenerating] = useState(false);
  const [showRightSidebar, setShowRightSidebar] = useState(false);
  const [showLeftSidebar, setShowLeftSidebar] = useState(() => window.innerWidth >= 768);

  useEffect(() => {
    if (isMobile) setShowLeftSidebar(false);
  }, [isMobile]);

  // Mobile UI Toggle
  const [mobileShowList, setMobileShowList] = useState(false);

  // Modals
  const [isCreatePlaylistModalOpen, setIsCreatePlaylistModalOpen] = useState(false);
  const [isAddToPlaylistModalOpen, setIsAddToPlaylistModalOpen] = useState(false);
  const [songToAddToPlaylist, setSongToAddToPlaylist] = useState<Song | null>(null);

  // Video Modal

  // Cover regen modal — manual Pollinations / upload entry from SongList row
  // and RightSidebar. Updates songs.cover_url via /api/songs/:id/regen-cover.
  const [songForCoverRegen, setSongForCoverRegen] = useState<Song | null>(null);
  const [songForReplay, setSongForReplay] = useState<Song | null>(null);
  const [replayRequestRef, setReplayRequestRef] = useState('');
  const [songToProcess, setSongToProcess] = useState<Song | null>(null);
  const [songForVideo, setSongForVideo] = useState<Song | null>(null);

  // Settings Modal
  const [showSettingsModal, setShowSettingsModal] = useState(false);
  // Which settings page to land on, when something asks for a particular one.
  const [settingsSection, setSettingsSection] = useState<string | null>(null);

  // Profile View

  // Song View

  // Playlist View
  const [viewingPlaylistId, setViewingPlaylistId] = useState<string | null>(null);

  // Reuse State
  const [reuseData, setReuseData] = useState<{ song: Song, timestamp: number } | null>(null);

  const audioRef = useRef<HTMLAudioElement | null>(null);
  const selectedSongRef = useRef<Song | null>(null);
  const currentSongIdRef = useRef<string | null>(null);
  const pendingSeekRef = useRef<number | null>(null);
  const playNextRef = useRef<() => void>(() => {});

  // Mobile Details Modal State
  const [showMobileDetails, setShowMobileDetails] = useState(false);

  // Toast State
  const [toast, setToast] = useState<{ message: string; type: ToastType; isVisible: boolean }>({
    message: '',
    type: 'success',
    isVisible: false,
  });

  // Confirm Dialog State
  const [confirmDialog, setConfirmDialog] = useState<{
    title: string;
    message: string;
    /** The word on the button that goes through; the author's own when left out. */
    confirmLabel?: string;
    onConfirm: () => void;
  } | null>(null);


  // Every message the studio or an agent reports, newest last. The control
  // strip's bell reads them back once the toast itself has gone.
  // The log outlives a reload: what the window reported is kept in local storage,
  // the way the theme and the language are. Only the newest 200 are kept.
  const [agentNotices, setAgentNotices] = useState<AgentNotice[]>(() => {
    try {
      const saved: unknown = JSON.parse(localStorage.getItem('messages') ?? '[]');
      return Array.isArray(saved) ? (saved as AgentNotice[]).slice(-200) : [];
    } catch {
      return [];
    }
  });
  // Ids continue after the restored ones, so the list keys stay unique.
  const nextNoticeId = useRef(agentNotices.reduce((top, notice) => Math.max(top, notice.id), 0));

  // Everything the window reports goes through here: the studio's own messages
  // and, over the MCP bridge below, the ones an agent sends. Each is also kept
  // for the message log, which tells the two apart by their source.
  const showToast = (message: string, type: ToastType = 'success', source: AgentNotice['source'] = 'studio') => {
    setToast({ message, type, isVisible: true });
    nextNoticeId.current += 1;
    setAgentNotices(prev => [...prev.slice(-199), {
      id: nextNoticeId.current,
      text: message,
      tone: type,
      source,
      at: Date.now(),
    }]);
  };

  // Not every report deserves a toast: the message log keeps the record while the
  // window stays quiet. The agent's questions and the answers they get go there.
  const noteJournal = (message: string, source: AgentNotice['source'] = 'studio') => {
    nextNoticeId.current += 1;
    setAgentNotices(prev => [...prev.slice(-199), {
      id: nextNoticeId.current,
      text: message,
      tone: 'info',
      source,
      at: Date.now(),
    }]);
  };

  useEffect(() => {
    try {
      localStorage.setItem('messages', JSON.stringify(agentNotices));
    } catch {
      // Storage can be full or unavailable; the log simply stops outliving a reload.
    }
  }, [agentNotices]);

  // Each entry carries its own cross: this drops that one from the log.
  const removeNotice = (id: number) => {
    setAgentNotices(prev => prev.filter(notice => notice.id !== id));
  };

  /** A clean log, the past included: the person asked for it, and the history is theirs. */
  const clearJournal = () => {
    setAgentNotices([]);
  };

  /** A clean log is asked for, not taken: the past cannot be brought back, so the window puts
   * the question first and the person decides. */
  const askClearJournal = () => {
    setConfirmDialog({
      title: t('journalClearTitle'),
      message: t('journalClearMessage'),
      confirmLabel: t('journalClearConfirm'),
      onConfirm: () => {
        setConfirmDialog(null);
        clearJournal();
      },
    });
  };

  // ---------------------------------------------------------------- sessions
  // Sessions live in the service: one set for the window and for the agent, so
  // both see the same sessions and a track knows which one it was made in.
  // A session that was closed stays closed, because the closed time is stored.
  const [sessions, setSessions] = useState<Workspace[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  /* The request a person made before there was any session to make it in: it waits here
     until the session exists, then runs - nothing is generated into the void. */
  const pendingGenerateRef = useRef<(YueRequest & { _tempId?: string }) | null>(null);
  /* How many items one page of a list holds: a habit of the person, kept like the theme
     and the volume, so a long list is read at their own pace. */
  const [itemsPerPage, setItemsPerPage] = useState<number>(() => {
    const stored = Number(localStorage.getItem('itemsPerPage'));
    return [10, 25, 50].includes(stored) ? stored : 25;
  });
  useEffect(() => { localStorage.setItem('itemsPerPage', String(itemsPerPage)); }, [itemsPerPage]);
  /* What the lists are read by - the day something was made, or its name - and which
     way round. A habit of the person, kept like the theme and the page size. */
  const [listOrder, setListOrder] = useState<SortOrder>(() => {
    try {
      const stored = JSON.parse(localStorage.getItem('listOrder') ?? 'null') as SortOrder | null;
      if (stored && ['created', 'updated', 'name'].includes(stored.by) && typeof stored.descending === 'boolean') return stored;
    } catch {
      // a value we cannot read is simply forgotten
    }
    return { by: 'created', descending: true };
  });
  useEffect(() => { localStorage.setItem('listOrder', JSON.stringify(listOrder)); }, [listOrder]);
  const [sessionsReady, setSessionsReady] = useState(false);
  const [sessionView, setSessionView] = useState<SessionView>('session');
  // The list column shows either the tracks or the session browser.
  const [centerView, setCenterView] = useState<'tracks' | 'sessions'>('tracks');
  // Creating a session is the one modal: it asks for the name and gets out.
  const [isSessionCreateOpen, setIsSessionCreateOpen] = useState(false);
  /** A session just opened from the library page: offer to step over to where it is worked on. */
  const [editModeOffer, setEditModeOffer] = useState<{ sessionName: string } | null>(null);
  // The agent's request to make a track waits here until the user answers it.
  const [agentRequest, setAgentRequest] = useState<{
    sessionId: string;
    sessionName: string;
    title?: string;
    /** The answer the server is waiting for. */
    answer: (decision: string) => void;
  } | null>(null);

  // The agent's request to do something that cannot be undone waits here until
  // the user answers it: the window is the only place that may say yes.
  const [agentPermission, setAgentPermission] = useState<AgentPermissionRequest | null>(null);
  const [sessionChoice, setSessionChoice] = useState<SessionChoiceRequest | null>(null);
  /** Read the sessions from the service: the one source both sides look at. */
  const readSessions = useCallback(async () => {
    try {
      const [list, active] = await Promise.all([loadWorkspaces(), activeWorkspace()]);
      setSessions(list);
      setActiveSessionId(active?.id ?? null);
    } catch {
      // The service is not answering: the list simply stays empty, with the
      // studio's own "open or create a session" line in it. No alarm is raised:
      // a build older than these routes looks exactly the same as a silence.
    } finally {
      setSessionsReady(true);
    }
  }, []);

  useEffect(() => {
    void readSessions();
  }, [readSessions]);

  /**
   * The tracks the library already had when the window first looked at it.
   * Only a track that appears AFTER that moment belongs to the session that is
   * open then: opening a session must not swallow the shelf it was opened next
   * to. Everything older is not lost - it simply is not this session's.
   */
  const knownTrackIds = useRef<Set<string> | null>(null);

  /**
   * Tracks from before sessions existed have no session to belong to. The SERVICE gathers
   * them into the session it keeps for exactly this, and marks that session, so two windows
   * asking at once get one session between them. The window used to search for a session by
   * its own word for "Import" and make one when it did not find it - and with two windows
   * open, each reading a different list, that is how two sessions of the same kind appeared.
   * `importNames` is every word the window may have written into an older library, so the
   * service adopts the session that is already there instead of making another.
   */
  const importNames = useMemo(
    () => [...new Set(Object.values(yue2).map((words) => words.sessionImportName))],
    [],
  );
  const importedOnce = useRef(false);

  useEffect(() => {
    if (importedOnce.current || !sessionsReady || songs.length === 0) return;
    const known = new Set(sessions.flatMap((session) => session.songIds));
    const orphans = songs.filter((song) => !known.has(song.id));
    if (orphans.length === 0) {
      importedOnce.current = true;
      return;
    }
    importedOnce.current = true;
    void adoptImportSession(importNames)
      .then(() => readSessions())
      .catch(() => undefined);
  }, [sessionsReady, sessions, songs, readSessions, importNames]);

  /**
   * A track that appeared belongs to the session that was open: it is written
   * into that session, so the pair survives a reload and the agent sees it too.
   */
  useEffect(() => {
    if (knownTrackIds.current === null) {
      // The first look at the library is not an arrival: what is there stays put.
      if (songs.length > 0) knownTrackIds.current = new Set(songs.map((song) => song.id));
      return;
    }
    const fresh = songs
      .filter((song) => !knownTrackIds.current!.has(song.id))
      .map((song) => song.id);
    if (fresh.length === 0) return;
    for (const id of fresh) knownTrackIds.current.add(id);
    if (!sessionsReady || !activeSessionId) return;
    const session = sessions.find((item) => item.id === activeSessionId);
    if (!session) return;
    const missing = fresh.filter((id) => !session.songIds.includes(id));
    if (missing.length === 0) return;
    void updateWorkspace(session.id, session.name, [...session.songIds, ...missing])
      .then(() => readSessions())
      .catch(() => undefined);
  }, [songs, sessions, activeSessionId, sessionsReady, readSessions]);

  /**
   * How the queue in front of you plays back is remembered per context - the
   * session, the library, a playlist - so a choice made in one place does not
   * follow you into another. The service keeps it; the window follows.
   */
  const playbackContext = sessionView === 'library'
    ? 'library:all'
    : (activeSessionId ? `session:${activeSessionId}` : 'single');

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const mode = await readPlaybackMode(playbackContext);
        if (!alive) return;
        // A context nobody has chosen for yet plays the way the studio starts -
        // repeat all, no shuffle. Keeping the previous context's choice instead
        // was the "my setting followed me into the library" bug the modes exist
        // to prevent.
        setRepeatMode(mode ? mode.repeatMode : 'all');
        setIsShuffle(mode ? mode.shuffle : false);
      } catch {
        // The service is away: the window keeps the settings it already has.
      }
    })();
    return () => { alive = false; };
  }, [playbackContext]);

  /** The buttons in the player write the choice back for this context. */
  const rememberPlaybackMode = (next: { repeatMode?: RepeatMode; shuffle?: boolean }) => {
    void writePlaybackMode(playbackContext, {
      repeatMode: next.repeatMode ?? repeatMode,
      shuffle: next.shuffle ?? isShuffle,
    }).catch(() => undefined);
  };

  const openSession = sessions.find((session) => session.id === activeSessionId) ?? null;
  /**
   * How many of a session's tracks are here, counted by the session itself.
   * A shared map of "which session a track is in" was the wrong shape: a track
   * that sits in two sessions was counted for one of them and lost to the other.
   */
  const tracksInSession = (sessionId: string) => {
    const session = sessions.find((item) => item.id === sessionId);
    if (!session) return 0;
    // A part (a stem) is not a song: it is shown inside the song it came from, so it is not
    // counted - the same rule the song list follows, so the two lists agree on the number.
    return splitByParent(songs.filter((song) => session.songIds.includes(song.id))).roots.length;
  };
  // What the list shows: the open session's own tracks, or every track there is.
  const visibleSongs = sessionView === 'library'
    ? songs
    : songs.filter((song) => (openSession?.songIds ?? []).includes(song.id));

  // One browser, two places: the list column (opened by the ☰ button) and the
  // library's "Sessions" tab. Both must behave the same, so they share one element.
  const sessionBrowser = (
    <SessionList
      sessions={sessions}
      activeSessionId={activeSessionId}
      trackCount={tracksInSession}
      itemsPerPage={itemsPerPage}
      order={listOrder}
      onOpenSession={(id) => {
        setSessionView('session');
        setCenterView('tracks');
        // The service decides: opening one session closes the one before it.
        void openWorkspace(id)
          .then(() => readSessions())
          .catch(() => undefined);
        // On the library page the result of that click is a list of sessions, not the
        // session itself, so the window offers to walk over to the page with the alpha
        // panel - the one where a session is actually worked on.
        if (currentView === 'library') {
          const target = sessions.find((session) => session.id === id);
          setEditModeOffer({ sessionName: target ? sessionTitle(target, t('sessionImportName')) : '' });
        }
      }}
      onCloseSession={(id) => {
        void closeWorkspace(id)
          .then(() => readSessions())
          .catch(() => undefined);
      }}
      onRenameSession={(id, name) => {
        const session = sessions.find((item) => item.id === id);
        if (!session) return;
        void updateWorkspace(id, name, session.songIds)
          .then(() => readSessions())
          .catch(() => undefined);
      }}
      onCreateRequest={() => setIsSessionCreateOpen(true)}
    />
  );

  // Where an agent's track may be made: the studio asks the person, because a track
  // belongs in one session and only they open sessions. "Always" inside a session
  // answers it silently from then on; otherwise the answer goes back to the server,
  // which lets the call go on or refuses it.
  useBridgeCommand('session_confirm', (args) => new Promise<{ decision: string }>((resolve) => {
    const tool = String(args.tool ?? '');
    const title = String(args.title ?? '');
    const sessionId = String(args.session_id ?? '');
    const sessionName = String(args.session_name ?? '');
    const answer = (decision: string) => {
      setAgentRequest(null);
      resolve({ decision });
    };
    // The log names the track the question is about: the tool's own name says nothing
    // to the person reading it.
    noteJournal((title ? t('agentJournalAskNamed') : t('agentJournalAsk'))
      .replace('{kind}', t('journalKindSong'))
      .replace('{name}', title), 'agent');

    setAgentRequest({ sessionId, sessionName, title, answer });
  }));

  const closeToast = () => {
    setToast(prev => ({ ...prev, isVisible: false }));
  };

  const refreshNativeLibrary = useCallback(async (): Promise<boolean> => {
    try {
      const [nativeSongs, nativePlaylists] = await Promise.all([loadNativeLibrarySongs(), loadNativePlaylists()]);
      setSongs(prev => {
        const generatingSongs = prev.filter(song => song.isGenerating);
        return [...generatingSongs, ...nativeSongs];
      });
      setPlaylists(nativePlaylists);
      // A fresh native library is still the authoritative store. Falling back
      // to the retired ACE service when it is empty made ordinary first-run
      // actions issue requests to a server that is not part of this desktop app.
      return true;
    } catch {
      return false;
    }
  }, []);
  /**
   * Likes used to be this window's private list in local storage. They are
   * moved into the songs themselves once (the service keeps them there now),
   * so a like stops being one window's opinion and survives a reload.
   */
  useEffect(() => {
    const stored = loadNativeLikedSongIds();
    if (stored.size === 0) return;
    let cancelled = false;
    void (async () => {
      const moved: string[] = [];
      for (const id of stored) {
        try {
          await setNativeSongLiked(id, true);
          moved.push(id);
        } catch {
          // the song is gone: its like went with it
        }
      }
      if (cancelled) return;
      localStorage.removeItem(NATIVE_LIKED_SONG_IDS_KEY);
      localStorage.setItem(NATIVE_LIKED_SONG_IDS_KEY + '-moved', JSON.stringify(moved.length));
      await refreshNativeLibrary();
    })();
    return () => { cancelled = true; };
  }, [refreshNativeLibrary]);


  // The library asked for while the service was still starting came back
  // empty; once the service answers again it is read afresh.
  const wasOffline = useRef(false);
  useEffect(() => {
    if (nativeModels === 'offline') {
      wasOffline.current = true;
      return;
    }
    if (wasOffline.current && nativeModels !== 'unknown') {
      wasOffline.current = false;
      void refreshNativeLibrary();
    }
  }, [nativeModels, refreshNativeLibrary]);

  const handleNativeReplay = useCallback((song: Song) => {
    if (!song.nativeReplayAvailable) return;
    const ref = `replay_${Date.now()}_${Math.random().toString(36).slice(2, 11)}`;
    ownRequestsRef.current.add(ref);
    setReplayRequestRef(ref);
    setSongForReplay(song);
  }, []);
  const closeReplay = useCallback(() => {
    ownRequestsRef.current.delete(replayRequestRef);
    setSongForReplay(null);
  }, [replayRequestRef]);

  // Keep selectedSongRef in sync for use in callbacks without stale closures
  useEffect(() => { selectedSongRef.current = selectedSong; }, [selectedSong]);

  // Cleanup active jobs on unmount
  useEffect(() => {
    return () => {
      // Clear all polling intervals when component unmounts
      activeJobsRef.current.forEach(({ pollInterval }) => {
        clearInterval(pollInterval);
      });
      activeJobsRef.current.clear();
    };
  }, []);

  const handleShowDetails = (song: Song) => {
    setSelectedSong(song);
    setShowMobileDetails(true);
  };

  // Reuse Handler
  const handleReuse = (song: Song) => {
    setReuseData({ song, timestamp: Date.now() });
    setCurrentView('create');
    setMobileShowList(false);
  };

  // Song Update Handler
  const handleSongUpdate = (updatedSong: Song) => {
    setSongs(prev => prev.map(s => s.id === updatedSong.id ? updatedSong : s));
    if (currentSong?.id === updatedSong.id) {
      setCurrentSong(updatedSong);
    }
    if (selectedSong?.id === updatedSong.id) {
      setSelectedSong(updatedSong);
    }
  };

  // Theme Effect
  useEffect(() => {
    localStorage.setItem('theme', theme);
    if (theme === 'dark') {
      document.documentElement.classList.add('dark');
    } else {
      document.documentElement.classList.remove('dark');
    }
  }, [theme]);

  const toggleTheme = () => {
    setTheme(prev => prev === 'dark' ? 'light' : 'dark');
  };

  // URL Routing Effect
  useEffect(() => {
    const handleUrlChange = () => {
      const path = window.location.pathname;
      const params = new URLSearchParams(window.location.search);

      if (path === '/create' || path === '/') {
        setCurrentView('create');
        setMobileShowList(false);
      } else if (path === '/library') {
        setCurrentView('library');
      } else if (path.startsWith('/playlist/')) {
        const playlistId = path.substring(10);
        if (playlistId) {
          setViewingPlaylistId(playlistId);
          setCurrentView('playlist');
        }
      } else if (path === '/search') {
        setCurrentView('search');
      } else if (path === '/news') {
        setCurrentView('news');
      }
    };

    handleUrlChange();

    window.addEventListener('popstate', handleUrlChange);
    return () => window.removeEventListener('popstate', handleUrlChange);
  }, []);

  // Load the native library once at start; changes arrive as yue:library-changed.
  useEffect(() => {
    void refreshNativeLibrary();
  }, [refreshNativeLibrary]);


  // Player Logic
  /// The queue the user actually built: a playlist, or the one track they
  /// clicked. The whole library is NOT a queue - treating it as one turned
  /// "play this track" into a shadow playlist that ran on through every stem
  /// of the same song, and it went on even when the player was stopped.
  const getActiveQueue = (song?: Song) => {
    if (playQueue.length > 0) return playQueue;
    return song ? [song] : [];
  };

  const playNext = useCallback(() => {
    if (!currentSong) return;
    const queue = getActiveQueue(currentSong);
    if (queue.length === 0) return;

    const currentIndex = queueIndex >= 0 && queue[queueIndex]?.id === currentSong.id
      ? queueIndex
      : queue.findIndex(s => s.id === currentSong.id);
    if (currentIndex === -1) return;

    if (repeatMode === 'one') {
      if (audioRef.current) {
        audioRef.current.currentTime = 0;
        audioRef.current.play();
      }
      return;
    }

    // Find next playable song (has audioUrl and not generating)
    const queueLen = queue.length;
    for (let i = 1; i <= queueLen; i++) {
      let nextIndex;
      if (isShuffle) {
        nextIndex = Math.floor(Math.random() * queueLen);
        if (queueLen > 1 && nextIndex === currentIndex) continue;
      } else {
        nextIndex = currentIndex + i;
        // In 'none' repeat mode, stop at end of queue
        if (repeatMode === 'none' && nextIndex >= queueLen) {
          setIsPlaying(false);
          return;
        }
        nextIndex = nextIndex % queueLen;
      }

      const candidate = queue[nextIndex];
      if (candidate.audioUrl && !candidate.isGenerating) {
        setQueueIndex(nextIndex);
        setCurrentSong(candidate);
        setIsPlaying(true);
        return;
      }
    }

    // No playable songs found
    setIsPlaying(false);
  }, [currentSong, queueIndex, isShuffle, repeatMode, playQueue, songs]);

  const playPrevious = useCallback(() => {
    if (!currentSong) return;
    const queue = getActiveQueue(currentSong);
    if (queue.length === 0) return;

    const currentIndex = queueIndex >= 0 && queue[queueIndex]?.id === currentSong.id
      ? queueIndex
      : queue.findIndex(s => s.id === currentSong.id);
    if (currentIndex === -1) return;

    if (currentTime > 3) {
      if (audioRef.current) audioRef.current.currentTime = 0;
      return;
    }

    // Find previous playable song (has audioUrl and not generating)
    const queueLen = queue.length;
    for (let i = 1; i <= queueLen; i++) {
      let prevIndex;
      if (isShuffle) {
        prevIndex = Math.floor(Math.random() * queueLen);
        if (queueLen > 1 && prevIndex === currentIndex) continue;
      } else {
        prevIndex = currentIndex - i;
        // In 'none' repeat mode, stop at beginning of queue
        if (repeatMode === 'none' && prevIndex < 0) {
          if (audioRef.current) audioRef.current.currentTime = 0;
          return;
        }
        prevIndex = (prevIndex + queueLen) % queueLen;
      }

      const candidate = queue[prevIndex];
      if (candidate.audioUrl && !candidate.isGenerating) {
        setQueueIndex(prevIndex);
        setCurrentSong(candidate);
        setIsPlaying(true);
        return;
      }
    }

    // No playable songs found
    setIsPlaying(false);
  }, [currentSong, queueIndex, currentTime, isShuffle, repeatMode, playQueue, songs]);

  useEffect(() => {
    playNextRef.current = playNext;
  }, [playNext]);

  // Audio Setup
  useEffect(() => {
    audioRef.current = new Audio();
    audioRef.current.crossOrigin = "anonymous";
    const audio = audioRef.current;
    audio.volume = volume;
    // the equalizer and the visualisers hear the player through a Web Audio graph made when first needed
    registerPlayer(audio);

    const onTimeUpdate = () => setCurrentTime(audio.currentTime);
    const applyPendingSeek = () => {
      if (pendingSeekRef.current === null) return;
      if (audio.seekable.length === 0) return;
      const target = pendingSeekRef.current;
      const safeTarget = Number.isFinite(audio.duration)
        ? Math.min(Math.max(target, 0), audio.duration)
        : Math.max(target, 0);
      audio.currentTime = safeTarget;
      setCurrentTime(safeTarget);
      pendingSeekRef.current = null;
    };

    const onLoadedMetadata = () => {
      setDuration(audio.duration);
      applyPendingSeek();
    };

    const onCanPlay = () => {
      applyPendingSeek();
    };

    const onProgress = () => {
      applyPendingSeek();
    };

    const onEnded = () => {
      if (repeatModeRef.current === 'stop') {
        setIsPlaying(false);
        return;
      }
      playNextRef.current();
    };

    const onError = (e: Event) => {
      if (audio.error && audio.error.code !== 1) {
        console.error("Audio playback error:", audio.error);
        if (audio.error.code === 4) {
          showToast(t('songNotAvailable'), 'error');
        } else {
          showToast(t('unableToPlay'), 'error');
        }
      }
      setIsPlaying(false);
    };

    audio.addEventListener('timeupdate', onTimeUpdate);
    audio.addEventListener('loadedmetadata', onLoadedMetadata);
    audio.addEventListener('canplay', onCanPlay);
    audio.addEventListener('progress', onProgress);
    audio.addEventListener('ended', onEnded);
    audio.addEventListener('error', onError);

    return () => {
      audio.pause();
      audio.removeEventListener('timeupdate', onTimeUpdate);
      audio.removeEventListener('loadedmetadata', onLoadedMetadata);
      audio.removeEventListener('canplay', onCanPlay);
      audio.removeEventListener('progress', onProgress);
      audio.removeEventListener('ended', onEnded);
      audio.removeEventListener('error', onError);
    };
  }, []);

  // Handle Playback State
  useEffect(() => {
    const audio = audioRef.current;
    if (!audio || !currentSong?.audioUrl) return;

    const playAudio = async () => {
      try {
        if (audioGraph()) await resumeAudioGraph();
        await audio.play();
      } catch (err) {
        if (err instanceof Error && err.name !== 'AbortError') {
          console.error("Playback failed:", err);
          if (err.name === 'NotSupportedError') {
            showToast(t('songNotAvailable'), 'error');
          }
          setIsPlaying(false);
        }
      }
    };

    if (currentSongIdRef.current !== currentSong.id) {
      currentSongIdRef.current = currentSong.id;
      audio.src = currentSong.audioUrl;
      audio.load();
      if (isPlaying) playAudio();
    } else {
      if (isPlaying) playAudio();
      else audio.pause();
    }
  }, [currentSong, isPlaying]);

  // Handle Volume
  useEffect(() => {
    if (audioRef.current) {
      audioRef.current.volume = volume;
    }
    localStorage.setItem('volume', String(volume));
  }, [volume]);

  // Handle Playback Rate
  useEffect(() => {
    if (audioRef.current) {
      audioRef.current.playbackRate = playbackRate;
    }
  }, [playbackRate]);

  // a visualiser in its own window hears the player through this window
  useEffect(() => serveVisualizerFeed(), []);

  // Spacebar play/pause
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.code !== 'Space' || winampOn()) return;
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || (e.target as HTMLElement)?.isContentEditable) return;
      e.preventDefault();
      if (currentSong) {
        if (currentSong.audioUrl) {
          setIsPlaying(prev => !prev);
        }
      } else {
        // No song selected — play first available
        const available = songs.filter(s => s.audioUrl && !s.isGenerating);
        if (available.length > 0) {
          playSong(available[0], available);
        }
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [currentSong, songs]);

  // Helper to cleanup a job and check if all jobs are done
  const cleanupJob = useCallback((jobId: string, tempId: string) => {
    const jobData = activeJobsRef.current.get(jobId);
    if (jobData) {
      clearInterval(jobData.pollInterval);
      activeJobsRef.current.delete(jobId);
    }

    // Remove temp song
    setSongs(prev => prev.filter(s => s.id !== tempId));

    // Update active job count
    setActiveJobCount(activeJobsRef.current.size);

    // If no more active jobs, set isGenerating to false
    if (activeJobsRef.current.size === 0) {
      setIsGenerating(false);
    }
    drainQueueWaiters();
  }, []);

  // Cancel a single generation. The `id` may be either:
  //  - a backend jobId (track is past pre-flight, audio gen is running) → POST /cancel
  //  - a pre-flight tempId (still in OpenRouter LLM call, no jobId yet)  → abort the
  //    registered AbortController, drop the placeholder card, release slot
  //
  // We unify both paths under one handler because the SongList row only knows
  // `song.id` (= tempId) and `song.jobId`; a pre-flight card has tempId but
  // no jobId, so the cancel button passes whatever it has and we figure it
  // out here.
  /// One handler for both card kinds: a pre-flight placeholder only has a
  /// tempId (no engine job yet, so there is nothing to cancel remotely), while
  /// a submitted card carries the mm-server job id.
  const stopEngineJob = useCallback(async (jobId: string) => {
    try {
      await fetch(`/v1/music/jobs/${encodeURIComponent(jobId)}`, { method: 'POST' });
    } catch (error) {
      console.error('Cancel request failed:', error);
    }
  }, []);

  const cancelGeneration = useCallback(async (id: string) => {
    const preflightAc = preflightAbortersRef.current.get(id);
    if (preflightAc) {
      preflightAc.abort();
      preflightAbortersRef.current.delete(id);
      setSongs(prev => prev.map(song => song.id === id ? { ...song, isGenerating: false, stage: 'cancelled' } : song));
      decrementPendingClicks(1);
      drainQueueWaiters();
      return;
    }

    await stopEngineJob(id);
    const jobData = activeJobsRef.current.get(id);
    if (jobData) {
      clearInterval(jobData.pollInterval);
      activeJobsRef.current.delete(id);
      setActiveJobCount(activeJobsRef.current.size);
      if (activeJobsRef.current.size === 0) setIsGenerating(false);
      drainQueueWaiters();
      setSongs(prev => prev.map(song =>
        song.id === jobData.tempId ? { ...song, isGenerating: false, stage: 'cancelled' } : song
      ));
    }
  }, [drainQueueWaiters, decrementPendingClicks, stopEngineJob]);

  /// Reset drops the card as well as the job: the engine is asked to stop, then
  /// the placeholder is removed so the list matches reality.
  const resetSingleJob = useCallback(async (id: string) => {
    const jobData = activeJobsRef.current.get(id);
    if (!jobData) {
      const aborter = preflightAbortersRef.current.get(id);
      if (aborter) {
        aborter.abort();
        preflightAbortersRef.current.delete(id);
      }
      setSongs(prev => prev.filter(song => song.id !== id));
      drainQueueWaiters();
      return;
    }

    await stopEngineJob(id);
    clearInterval(jobData.pollInterval);
    activeJobsRef.current.delete(id);
    setSongs(prev => prev.filter(song => song.id !== jobData.tempId));
    setActiveJobCount(activeJobsRef.current.size);
    if (activeJobsRef.current.size === 0) setIsGenerating(false);
    drainQueueWaiters();
  }, [drainQueueWaiters, stopEngineJob]);

  const cancelAllGenerations = useCallback(async () => {
    preflightAbortersRef.current.forEach(aborter => aborter.abort());
    preflightAbortersRef.current.clear();

    // "Without stopping" would send the form again the moment the queue empties
    window.dispatchEvent(new CustomEvent('yue:cancel-all'));
    const running = [...activeJobsRef.current.entries()];
    await Promise.all(running.map(([jobId]) => stopEngineJob(jobId)));
    // The service's list, not only this window's: a request whose answer is
    // still on its way back is on neither list here, and would run to the end.
    const response = await fetch('/v1/music/jobs');
    if (response.ok) {
      const listed: YueJob[] = await response.json();
      await Promise.all(listed.filter(job => !activeJobsRef.current.has(job.id)).map(job => stopEngineJob(job.id)));
    } else {
      console.error(`Cancel all: the running jobs did not load (${response.status})`);
    }
    running.forEach(([, { pollInterval }]) => clearInterval(pollInterval));
    const tempIds = new Set(running.map(([, job]) => job.tempId));
    activeJobsRef.current.clear();
    setSongs(prev => prev.filter(song => !tempIds.has(song.id) && !(song.isGenerating && !song.jobId)));
    setActiveJobCount(0);
    setIsGenerating(false);
    drainQueueWaiters();
    setPendingClickCount(0);
  }, [drainQueueWaiters, stopEngineJob]);

  const resetGeneration = cancelAllGenerations;

  // Refresh songs list (called when any job completes successfully)
  const refreshSongsList = useCallback(async () => {
    await refreshNativeLibrary();
  }, [refreshNativeLibrary]);

  /// Job phases mapped onto the studio's stage labels. yue-server
  /// reports a phase rather than a percentage, so the card shows an honest
  /// stage name and an indeterminate bar instead of a fabricated progress
  /// number.
  const beginPollingJob = useCallback((jobId: string, tempId: string) => {
    if (activeJobsRef.current.has(jobId)) return;

    const pollInterval = setInterval(async () => {
      try {
        const response = await fetch(`/v1/music/jobs/${encodeURIComponent(jobId)}`);
        if (!response.ok) throw new Error(`Job status request failed (${response.status})`);
        const job: YueJob = await response.json();

        // The stage is not set from this per-job status: mm-server reports
        // every queued job as "running", so trusting it here would light every
        // card up as generating. The engine-log poll has the global view - it
        // knows which single job the engine is actually rendering - and owns the
        // generating-vs-waiting distinction. This poll only reacts to the
        // terminal states below.
        void job;

        if (job.status === 'completed') {
          cleanupJob(jobId, tempId);
          setSongs(prev => prev.filter(song => song.id !== tempId));
          await refreshSongsList();
          const finished = job.songs?.[0] ?? job.song;
          if (finished?.id) setSelectedSong(current => current?.id === tempId ? null : current);
          showToast(job.songs && job.songs.length > 1
            ? `${job.songs.length} ${t('tracksReady') || 'tracks ready'}`
            : (t('trackReady') || 'Track ready'));
          if (window.innerWidth < 768) setMobileShowList(true);
        } else if (job.status === 'failed' || job.status === 'cancelled') {
          cleanupJob(jobId, tempId);
          setSongs(prev => prev.filter(song => song.id !== tempId));
          showToast(job.message || `${t('generationFailed')}`, job.status === 'failed' ? 'error' : 'info');
        }
      } catch (error) {
        console.error(`Polling error for job ${jobId}:`, error);
        cleanupJob(jobId, tempId);
        setSongs(prev => prev.filter(song => song.id !== tempId));
        showToast(error instanceof Error ? error.message : String(error), 'error');
      }
    }, 1500);

    activeJobsRef.current.set(jobId, { tempId, pollInterval });
    setActiveJobCount(activeJobsRef.current.size);
  }, [cleanupJob, refreshSongsList, t]);

  /// Jobs keep running in the service when the window reloads, and an agent
  /// connected over MCP starts its own: both get a card, and the same poller
  /// lands their tracks. The service is asked when the window opens and each
  /// time it reports an agent's change.
  useEffect(() => {
    if (!nativeSetupReady) return;
    // a notice that lands while the service is being asked asks it once more
    let busy = false;
    let again = false;
    const adopt = async () => {
      if (busy) {
        again = true;
        return;
      }
      busy = true;
      try {
        do {
          again = false;
          const response = await fetch('/v1/music/jobs');
          if (!response.ok) throw new Error(`the running jobs did not load (${response.status})`);
          const jobs: YueJob[] = await response.json();
          const own = ownRequestsRef.current;
          const fresh = jobs.filter(job =>
            !activeJobsRef.current.has(job.id) && !(job.client_ref && own.has(job.client_ref)));
          if (fresh.length === 0) continue;
          setSongs(prev => [
            ...fresh.map(job => ({
              id: `restored_${job.id}`,
              title: job.title || t('generating') || 'Generating...',
              lyrics: job.lyrics || '',
              style: job.style || '',
              coverUrl: '',
              duration: '--:--',
              createdAt: new Date(),
              isGenerating: true,
              jobId: job.id,
              stage: 'stageWaitingInQueue',
              tags: ['yue2'],
            })),
            ...prev,
          ]);
          setIsGenerating(true);
          fresh.forEach(job => beginPollingJob(job.id, `restored_${job.id}`));
        } while (again);
      } catch (error) {
        console.error('[ERROR] adopting running jobs:', error);
      } finally {
        busy = false;
      }
    };
    const onChange = () => { void adopt(); };
    onChange();
    window.addEventListener('studio:jobs-changed', onChange);
    return () => window.removeEventListener('studio:jobs-changed', onChange);
  }, [nativeSetupReady, beginPollingJob, t]);

  /// yue-server reports every job as "running"; the studio service reads the
  /// engine log stage by stage and returns the running job's progress with it.
  useEffect(() => {
    if (activeJobCount === 0) return;
    let cancelled = false;
    const stageLabel: Record<YueProgress['stage'], string> = {
      score: 'stageScore',
      semantic: 'stageSemanticProgress',
      acoustic: 'stageAcoustic',
      decode: 'stageDecode',
      transcribe: 'stageTranscribe',
    };

    const poll = async () => {
      try {
        const response = await fetch('/v1/engine/logs');
        if (!response.ok) return;
        const body: { progress?: YueProgress | null } = await response.json();
        const progress = body.progress;
        if (cancelled || !progress || progress.stage === 'transcribe') return;
        // The engine renders one job at a time in submission order, so only
        // the oldest generating card is the one being worked on.
        setSongs(prev => {
          const generating = prev.filter(song => song.isGenerating && song.jobId);
          if (generating.length === 0) return prev;
          const active = generating.reduce((oldest, song) =>
            (song.createdAt?.getTime() ?? 0) < (oldest.createdAt?.getTime() ?? 0) ? song : oldest,
          );
          return prev.map(song => {
            if (!song.isGenerating || !song.jobId) return song;
            if (song.id === active.id) return { ...song, progress: progress.fraction, stage: stageLabel[progress.stage] };
            return { ...song, progress: 0, stage: 'stageWaitingInQueue' };
          });
        });
      } catch {
        // Progress detail is a nicety; the job status poll remains the truth.
      }
    };

    void poll();
    const timer = window.setInterval(poll, 1500);
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [activeJobCount]);

  /* A generation needs an open session, otherwise its track never shows up in the list:
     ask the service (not the window state) and, when there is none, ask for one first. */
  const ensureSessionForGeneration = async (): Promise<boolean> => {
    const active = await activeWorkspace().catch(() => null);
    if (active) {
      setActiveSessionId(active.id);
      return true;
    }
    setIsSessionCreateOpen(true);
    return false;
  };

  const handleGenerate = async (params: YueRequest & { _tempId?: string }) => {
    if (!(await ensureSessionForGeneration())) {
      pendingGenerateRef.current = params;
      showToast(t('sessionNeededForGeneration'), 'info');
      return;
    }
    const tempId = params._tempId || `temp_${Date.now()}_${Math.random().toString(36).slice(2, 11)}`;
    if (!params._tempId) {
      setSongs(prev => [{
        id: tempId,
        title: params.title?.trim() || t('generating') || 'Generating...',
        lyrics: params.lyrics || '',
        style: params.style || '',
        coverUrl: '',
        duration: '--:--',
        createdAt: new Date(),
        isGenerating: true,
        stage: 'stageWaitingInQueue',
        tags: ['yue2'],
      }, ...prev]);
    } else {
      setSongs(prev => prev.map(song => song.id === tempId
        ? { ...song, title: params.title?.trim() || song.title, style: params.style || song.style, lyrics: params.lyrics || song.lyrics }
        : song));
    }

    setIsGenerating(true);
    ownRequestsRef.current.add(tempId);
    try {
      const { _tempId, ...request } = params;
      const response = await fetch('/v1/music/jobs', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ ...request, client_ref: tempId }),
      });
      const job: YueJob & { error?: string; message?: string } = await response.json().catch(() => ({}) as YueJob);
      if (!response.ok || job.status === 'failed') {
        throw new Error(job.message || job.error || `The engine rejected this request (${response.status})`);
      }
      setSongs(prev => prev.map(song => song.id === tempId ? { ...song, jobId: job.id } : song));
      beginPollingJob(job.id, tempId);
      decrementPendingClicks(1);
    } catch (error) {
      console.error('Generation error:', error);
      setSongs(prev => prev.filter(song => song.id !== tempId));
      decrementPendingClicks(1);
      if (activeJobsRef.current.size === 0) setIsGenerating(false);
      showToast(error instanceof Error ? error.message : t('generationFailed'), 'error');
    } finally {
      ownRequestsRef.current.delete(tempId);
    }
  };


  const togglePlay = () => {
    const song = currentSong || selectedSong;
    if (!song) return;
    if (!song.audioUrl) {
      showToast(t('songNotAvailable'), 'error');
      return;
    }
    // If no currentSong yet, start playing the selected song
    if (!currentSong && song) {
      playSong(song);
      return;
    }
    setIsPlaying(!isPlaying);
  };

  const playFirst = () => {
    const available = songs.filter(s => s.audioUrl && !s.isGenerating);
    if (available.length > 0) {
      playSong(available[0], available);
    }
  };

  const playSong = (song: Song, list?: Song[]) => {
    // A list is a queue only when it is one the user built - a playlist, or a
    // selection they chose. Falling back to the whole library made "play this
    // track" behave like a playlist of every song (and every stem) there is.
    const nextQueue = list && list.length > 0
      ? list
      : (playQueue.length > 0 && playQueue.some(s => s.id === song.id))
          ? playQueue
          : [song];
    const nextIndex = nextQueue.findIndex(s => s.id === song.id);
    setPlayQueue(nextQueue);
    setQueueIndex(nextIndex);

    if (currentSong?.id !== song.id) {
      const updatedSong = { ...song, viewCount: (song.viewCount || 0) + 1 };
      setCurrentSong(updatedSong);
      setSelectedSong(updatedSong);
      setIsPlaying(true);
      setSongs(prev => prev.map(s => s.id === song.id ? updatedSong : s));
    } else {
      togglePlay();
    }
    if (currentSong?.id === song.id) {
      setSelectedSong(song);
    }
    setShowRightSidebar(true);
  };

  // An agent connected over MCP works this window like a user
  const VIEWS: View[] = ['create', 'library', 'tools', 'adapters', 'playlist', 'search', 'news'];
  const songById = (id: unknown) => {
    const song = songs.find(entry => entry.id === id);
    if (!song) throw new Error(`No song ${String(id)} in the library; library_songs_list gives the ids.`);
    return song;
  };
  useBridgeCommand('navigate', ({ view }) => {
    if (!VIEWS.includes(view as View)) throw new Error(`Pages: ${VIEWS.join(', ')}.`);
    setCurrentView(view as View);
    return { text: `On ${String(view)}.` };
  });
  useBridgeCommand('notify', ({ text, tone }) => {
    const kind: ToastType = tone === 'error' || tone === 'success' ? tone : 'info';
    showToast(String(text ?? ''), kind, 'agent');
    return { text: 'Shown.' };
  });
  useBridgeCommand('open_settings', ({ section }) => {
    setSettingsSection(typeof section === 'string' ? section : null);
    setShowSettingsModal(true);
    return { text: 'Settings are open.' };
  });
  useBridgeCommand('player_state', () => winampControl() ? { winamp: true, ...winampControl()!.state() } : ({
    song: currentSong ? { id: currentSong.id, title: currentSong.title } : null,
    playing: isPlaying,
    position_seconds: Math.round(currentTime * 10) / 10,
    duration_seconds: Math.round(duration * 10) / 10,
    volume,
    shuffle: isShuffle,
    repeat: repeatMode,
    queue: playQueue.length,
  }));
  // while the Winamp mode is on, Winamp is the player the agent drives
  const inWinamp = async (change: Record<string, unknown>) => ({ text: `Winamp: ${await winampControl()!.set(change)}` });
  useBridgeCommand('library_liked', () => ({
    songs: songs.filter((song) => likedSongIds.has(song.id)).map((song) => ({ id: song.id, title: song.title, made: song.createdAt })),
  }));
  // The MCP server puts a question to the window and waits for the answer: the
  // agent never widens its own leash, only the person in front of the window.
  useBridgeCommand('agent_confirm', (args) => new Promise<{ decision: AgentPermissionDecision }>((resolve) => {
    const action = String(args.action ?? '');
    const scope = String(args.scope ?? 'always');
    // A removal is answered with one word: delete it, or do not - nothing is remembered.
    const removal = args.removal === true;
    const said: Record<string, TranslationKey> = removal
      ? { once: 'agentConfirmRemove', deny: 'agentConfirmCancel' }
      : {
        once: 'agentConfirmOnce',
        session: 'agentConfirmSession',
        always: 'agentConfirmAlways',
        ask: 'agentConfirmAsk',
        deny_always: 'agentConfirmDenyAlways',
      };
    // The log says what is about to happen, and to what: the tool's own name answered
    // nothing a person asked ("what is being removed, and whose?").
    const aboutName = String(args.target ?? '').trim();
    const aboutKind = t(kindOf(action));
    noteJournal((aboutName ? t('agentJournalAskNamed') : t('agentJournalAsk'))
      .replace('{kind}', aboutKind)
      .replace('{name}', aboutName), 'agent');
    setAgentPermission({
      action,
      scope,
      removal,
      target: removal ? String(args.target ?? '') : undefined,
      details: args.details as Record<string, unknown> | undefined,
      answer: (decision) => {
        noteJournal(t('agentJournalAnswer').replace('{answer}', t(said[decision] ?? 'agentConfirmCancel')));
        setAgentPermission(null);
        resolve({ decision });
      },
    });
  }));
  // The agent is about to make a track and nobody has said where the tracks go: the window
  // asks - the open session, a session per song, or one new session - and the answer, for
  // this work or for good, goes back to the server, which hands it on to the call.
  useBridgeCommand('session_choice', (args) => new Promise<SessionChoiceAnswer>((resolve) => {
    const tool = String(args.tool ?? '');
    const title = String(args.title ?? '');
    // Already the words themselves, not keys: the answer is written into the log as the
    // person would read it.
    const said: Record<string, string> = {
      current: t('sessionChoiceCurrent'),
      each: t('sessionChoiceEach'),
      deny: t('sessionChoiceDeny'),
    };
    // Same here: the question is about a track, and the log says so with the track's name.
    noteJournal((title ? t('agentJournalAskNamed') : t('agentJournalAsk'))
      .replace('{kind}', t('journalKindSong'))
      .replace('{name}', title), 'agent');
    setSessionChoice({
      tool,
      title,
      hasSession: args.has_session === true,
      sessionName: String(args.session_name ?? ''),
      answer: (answer) => {
        const word: string = answer.choice.startsWith('new:') ? answer.choice : (said[answer.choice] ?? answer.choice);
        noteJournal(t('agentJournalAnswer').replace('{answer}', word));
        setSessionChoice(null);
        resolve(answer);
      },
    });
  }));
  // The agent's own work lands in the message log quietly: no toast in the middle of the
  // screen, just a record a person can read - the act, the kind of thing it touched and that
  // thing's own name. The tool's name never reaches the log: "workspace_delete" tells nobody
  // what was deleted, which is exactly what the person needs to know.
  useBridgeCommand('journal_note', ({ verb, kind, target }) => {
    const did = String(verb ?? 'ran');
    const what = String(kind ?? 'other');
    const name = String(target ?? '').trim();
    const said = (word: string, fallback: TranslationKey) => t(JOURNAL_SAID[word] ?? fallback);
    noteJournal((name ? t('agentActionRanOn') : t('agentActionRan'))
      .replace('{verb}', said(did, 'journalVerbRan'))
      .replace('{kind}', said(what, 'journalKindOther'))
      .replace('{name}', name), 'agent');
    return 'Noted.';
  });
  useBridgeCommand('library_song_like', ({ song_id, liked }) => {
    const song = songById(song_id);
    if (likedSongIds.has(song.id) !== (liked !== false)) toggleLike(song.id);
    return { text: `${song.title} is ${liked !== false ? 'liked' : 'no longer liked'}.` };
  });
  useBridgeCommand('player_play', ({ song_id, song_ids, stem }) => {
    if (Array.isArray(song_ids) && song_ids.length) {
      // a list of songs becomes the queue, played from its first
      const list = song_ids.map((id) => songById(id)).filter((song) => song.audioUrl);
      if (!list.length) throw new Error('None of those songs has audio to play.');
      if (winampControl()) return inWinamp({ song_ids: list.map((song) => song.id) });
      playSong(list[0], list);
      return { text: `Playing ${list.length} song(s) from ${list[0].title}.` };
    }
    if (winampControl() && !stem) return inWinamp(song_id ? { song_id } : { action: 'play' });
    if (song_id && stem) {
      // one separated stem of the song, played on its own
      const song = songById(song_id);
      const take: Song = { ...song, id: `${song.id}#${String(stem)}`, title: `${song.title} · ${String(stem)}`, audioUrl: apiUrl(`/v1/library/songs/${encodeURIComponent(song.id)}/stems/${encodeURIComponent(String(stem))}`) };
      playSong(take, [take]);
      return { text: `Playing the ${String(stem)} stem of ${song.title}.` };
    }
    if (song_id) {
      const song = songById(song_id);
      if (currentSong?.id !== song.id) playSong(song);
      else setIsPlaying(true);
      return { text: `Playing ${song.title}.` };
    }
    if (!currentSong) throw new Error('Nothing is loaded; pass song_id.');
    setIsPlaying(true);
    return { text: `Playing ${currentSong.title}.` };
  });
  useBridgeCommand('player_pause', () => {
    if (winampControl()) return inWinamp({ action: 'pause' });
    setIsPlaying(false);
    return { text: 'Paused.' };
  });
  useBridgeCommand('player_seek', ({ seconds }) => {
    if (winampControl()) return inWinamp({ seek_seconds: Number(seconds) || 0 });
    handleSeek(Number(seconds) || 0);
    return { text: `At ${Number(seconds) || 0} s.` };
  });
  useBridgeCommand('player_next', () => {
    if (winampControl()) return inWinamp({ action: 'next' });
    playNext();
    return { text: 'Next song.' };
  });
  useBridgeCommand('player_previous', () => {
    if (winampControl()) return inWinamp({ action: 'previous' });
    playPrevious();
    return { text: 'Previous song.' };
  });
  useBridgeCommand('player_set', ({ volume: level, shuffle, repeat }) => {
    if (winampControl()) {
      return inWinamp({
        ...(level !== undefined ? { volume: Number(level) } : {}),
        ...(shuffle !== undefined ? { shuffle: Boolean(shuffle) } : {}),
        ...(repeat === 'none' || repeat === 'all' ? { repeat: repeat === 'all' } : {}),
      });
    }
    if (level !== undefined) setVolume(Math.max(0, Math.min(1, Number(level))));
    if (shuffle !== undefined) setIsShuffle(Boolean(shuffle));
    if (repeat === 'none' || repeat === 'all' || repeat === 'one' || repeat === 'stop') setRepeatMode(repeat);
    return { text: 'Set.' };
  });
  useBridgeCommand('video_open', ({ song_id }) => {
    const song = songById(song_id);
    setSongForVideo(song);
    return { text: `The video editor is open for ${song.title}.` };
  });
  useBridgeCommand('video_close', () => {
    setSongForVideo(null);
    return { text: 'The video editor is closed.' };
  });

  const handleSeek = (time: number) => {
    const audio = audioRef.current;
    if (!audio) return;
    if (Number.isNaN(audio.duration) || audio.readyState < 1 || audio.seekable.length === 0) {
      pendingSeekRef.current = time;
      return;
    }
    audio.currentTime = time;
    setCurrentTime(time);
  };

  /// Favourites are a local library flag: the desktop studio has no social
  /// service, so the star is persisted next to the library instead of being
  /// posted to a server that does not exist.
  /**
   * The thumbs-up goes to the studio's library, where the song keeps it: the
   * window shows the change at once and puts the mark back if the service
   * refuses, so the two never drift apart.
   */
  const toggleLike = (songId: string) => {
    const isLiked = likedSongIds.has(songId);
    const mark = (value: boolean) =>
      setSongs(prev => prev.map((song) => (song.id === songId ? { ...song, liked: value } : song)));
    mark(!isLiked);
    void setNativeSongLiked(songId, !isLiked).catch(() => mark(isLiked));
  };

  const handleDeleteSong = (song: Song) => {
    handleDeleteSongs([song]);
  };

  const handleDeleteSongs = (songsToDelete: Song[]) => {
    if (songsToDelete.length === 0) return;

    const isSingle = songsToDelete.length === 1;
    const title = isSingle ? t('confirmDeleteTitle') : t('confirmDeleteManyTitle');
    const message = isSingle
      ? t('deleteSongConfirm').replace('{title}', songsToDelete[0].title)
      : t('deleteSongsConfirm').replace('{count}', String(songsToDelete.length));

    setConfirmDialog({
      title,
      message,
      onConfirm: async () => {
        setConfirmDialog(null);

        const idsToDelete = new Set(songsToDelete.map(song => song.id));
        const succeeded: string[] = [];
        const failed: string[] = [];

        for (const song of songsToDelete) {
          try {
            await deleteNativeSong(song.id);
            succeeded.push(song.id);
          } catch (error) {
            console.error('Failed to delete song:', error);
            failed.push(song.id);
          }
        }

        if (succeeded.length > 0) {
          setSongs(prev => prev.filter(s => !idsToDelete.has(s.id) || failed.includes(s.id)));

          if (selectedSong?.id && succeeded.includes(selectedSong.id)) {
            setSelectedSong(null);
          }

          if (currentSong?.id && succeeded.includes(currentSong.id)) {
            setCurrentSong(null);
            setIsPlaying(false);
            if (audioRef.current) {
              audioRef.current.pause();
              audioRef.current.src = '';
            }
          }

          setPlayQueue(prev => prev.filter(s => !idsToDelete.has(s.id) || failed.includes(s.id)));
        }

        if (failed.length > 0) {
          showToast(t('songsDeletedPartial').replace('{succeeded}', String(succeeded.length)).replace('{total}', String(songsToDelete.length)), 'error');
        } else if (isSingle) {
          showToast(t('songDeleted'));
        } else {
          showToast(t('songsDeletedSuccess'));
        }
      },
    });
  };

  const createPlaylist = async (name: string, description: string) => {
    try {
      const playlist = await createNativePlaylist(name, description, songToAddToPlaylist ? [songToAddToPlaylist.id] : []);
      setPlaylists(prev => [playlist, ...prev]);
      if (songToAddToPlaylist) setSongToAddToPlaylist(null);
      showToast(t('playlistCreated'));
    } catch (error) {
      console.error('Create playlist error:', error);
      showToast(t('failedToCreatePlaylist'), 'error');
    }
  };

  const openAddToPlaylistModal = (song: Song) => {
    setSongToAddToPlaylist(song);
    setIsAddToPlaylistModalOpen(true);
  };

  const addSongToPlaylist = async (playlistId: string) => {
    if (!songToAddToPlaylist) return;
    try {
      const playlist = playlists.find(item => item.id === playlistId);
      if (!playlist) throw new Error('Playlist was not found in the local library');
      const songIds = Array.from(new Set([...(playlist.songIds || []), songToAddToPlaylist.id]));
      const updated = await updateNativePlaylist(playlist.id, playlist, songIds);
      setPlaylists(prev => prev.map(item => item.id === updated.id ? updated : item));
      setSongToAddToPlaylist(null);
      showToast(t('songAddedToPlaylist'));
    } catch (error) {
      console.error('Add song error:', error);
      showToast(t('failedToAddSong'), 'error');
    }
  };

  const handleNavigateToPlaylist = (playlistId: string) => {
    setViewingPlaylistId(playlistId);
    setCurrentView('playlist');
    window.history.pushState({}, '', `/playlist/${playlistId}`);
  };





  const handleBackFromPlaylist = () => {
    setViewingPlaylistId(null);
    setCurrentView('library');
    window.history.pushState({}, '', '/library');
  };

  const openCoverRegen = (song: Song) => {
    // Cover work is non-destructive, so playback deliberately keeps running.
    setSongForCoverRegen(song);
  };

  // Apply the new cover URL to local state without a full /api/songs reload
  // (the backend already wrote songs.cover_url; we just need the UI to
  // reflect it). Cache-bust by appending a timestamp so <img> re-fetches.
  const applyCoverUpdate = useCallback((songId: string, coverUrl: string) => {
    const bust = `${coverUrl}${coverUrl.includes('?') ? '&' : '?'}t=${Date.now()}`;
    setSongs(prev => prev.map(s => s.id === songId ? { ...s, coverUrl: bust } : s));
    setSelectedSong(prev => prev?.id === songId ? { ...prev, coverUrl: bust } : prev);
  }, []);

  // Every song written or removed, by this window or any other: six stems or a
  // batch delete arrive as one burst and are read once.
  useEffect(() => {
    let timer = 0;
    const reload = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => void refreshNativeLibrary(), 200);
    };
    window.addEventListener('yue:library-changed', reload);
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener('yue:library-changed', reload);
    };
  }, [refreshNativeLibrary]);

  // Background work reports its outcome here once, and the toast goes away.
  useEffect(() => {
    const onToast = (event: Event) => {
      const { message, type } = (event as CustomEvent<{ message: string; type?: ToastType }>).detail;
      showToast(message, type ?? 'info');
    };
    window.addEventListener('yue:toast', onToast);
    return () => window.removeEventListener('yue:toast', onToast);
  }, []);

  // A track sent to be covered, or a score to sing, opens the form it lands in
  // and waits there as a request until the form takes it.
  const [createRequest, setCreateRequest] = useState<CreateRequest | null>(null);
  useEffect(() => {
    const open = (event: Event) => {
      const detail = (event as CustomEvent).detail;
      if (event.type === 'yue:transcribe-song' && detail?.song) {
        setCreateRequest({ id: Date.now(), kind: 'transcribe', song: detail.song, melodyOnly: Boolean(detail.melodyOnly) });
      } else if (event.type === 'yue:use-score' && detail?.abc) {
        setCreateRequest({ id: Date.now(), kind: 'score', abc: detail.abc, cot: detail.cot, lyrics: detail.lyrics, title: detail.title });
      }
      setCurrentView('create');
      if (window.innerWidth < 768) setMobileShowList(false);
    };
    window.addEventListener('yue:transcribe-song', open);
    window.addEventListener('yue:use-score', open);
    return () => {
      window.removeEventListener('yue:transcribe-song', open);
      window.removeEventListener('yue:use-score', open);
    };
  }, []);

  // Render Layout Logic
  // The create page stays mounted once the engine is up, hidden while another
  // page shows: leaving it for the library used to throw away the style, the
  // lyrics, the score and every setting typed into it.
  const createKept = nativeModels !== 'offline' && nativeSetupReady;
  const showingCreate = !['tools', 'adapters', 'library', 'playlist', 'search', 'news'].includes(currentView);

  const renderContent = (view: typeof currentView = currentView) => {
    switch (view) {
      case 'tools':
        return <StudioToolsPanel initialSongId={stemsSongId} />;

      case 'adapters':
        return <AdaptersPage />;

      case 'library': {
        const allSongs = songs;
        return (
          <LibraryView
            itemsPerPage={itemsPerPage}
            allSongs={allSongs}
            likedSongs={songs.filter(s => likedSongIds.has(s.id))}
            playlists={playlists}
            onPlaySong={playSong}
            currentSong={currentSong}
            isPlaying={isPlaying}
            onCreatePlaylist={() => {
              setSongToAddToPlaylist(null);
              setIsCreatePlaylistModalOpen(true);
            }}
            onSelectPlaylist={(p) => handleNavigateToPlaylist(p.id)}
            onImported={() => { void refreshNativeLibrary(); }}
            sessionsContent={sessionBrowser}
          />
        );
      }

      case 'playlist':
        if (!viewingPlaylistId) return null;
        return (
          <PlaylistDetail
            playlistId={viewingPlaylistId}
            onBack={handleBackFromPlaylist}
            onPlaySong={playSong}
            onSelect={(s) => {
              setSelectedSong(s);
              setShowRightSidebar(true);
            }}
          />
        );

      case 'search':
        return (
          <SearchPage
            songs={songs}
            playlists={playlists}
            onPlaySong={playSong}
            currentSong={currentSong}
            isPlaying={isPlaying}
            onNavigateToPlaylist={handleNavigateToPlaylist}
          />
        );

      case 'news':
        return <NewsPage />;

      case 'create':
      default:
        // Two different situations, two different screens: nothing installed
        // is a decision to make, and an engine coming up is a wait to sit
        // through. They used to be the same page, which made a running studio
        // look like it owed 26 GB.
        if (nativeModels === 'offline') return <StudioOffline />;
        if (!nativeSetupReady) {
          // Until the studio has answered, and while the engine is coming up,
          // this is a wait - not a decision. The download page appears only
          // when components are genuinely missing.
          return nativeModels === 'missing'
            ? <SetupGate onReady={() => setNativeSetupReady(true)} />
            : <EngineStarting onReady={() => setNativeSetupReady(true)} />;
        }
        return (
          <div className="relative flex h-full min-h-0 min-w-0 w-full overflow-hidden">
            {/* Create Panel */}
            <div
              className={`
                ${mobileShowList ? 'hidden md:block' : 'w-full'}
                md:block min-h-0 min-w-0 shrink-0 h-full bg-zinc-50 dark:bg-suno-panel relative z-10 transition-colors duration-300
              `}
              style={{ width: window.innerWidth >= 768 ? leftPanel.width : undefined }}
            >
              <CreatePanel
                onGenerate={handleGenerate}
                isGenerating={isGenerating}
                activeJobCount={activeJobCount + pendingClickCount}
                initialData={reuseData}
                request={createRequest}
              />
            </div>
            {leftPanel.handle}

            {/* Song List */}
            <div className={`
              ${!mobileShowList ? 'hidden md:flex' : 'flex'}
              relative min-h-0 min-w-0 flex-1 flex-col h-full overflow-hidden bg-white dark:bg-suno transition-colors duration-300
            `}>
              {/* The control strip sits flush with the top of the list column */}
              <TopControlPanel
                view={sessionView}
                onViewChange={(next) => { setSessionView(next); setCenterView('tracks'); }}
                sessionsOpen={centerView === 'sessions'}
                onToggleSessions={() => setCenterView((prev) => (prev === 'sessions' ? 'tracks' : 'sessions'))}
                onRefresh={() => {
                  // Reading again by hand: the library, its playlists and the sessions.
                  void refreshNativeLibrary();
                  void readSessions();
                }}
                order={listOrder}
                onOrder={setListOrder}
                what={centerView === 'sessions' ? 'sessions' : 'songs'}
              />

              {centerView === 'sessions' ? (
                /* The session browser: a list, not a modal, so it can grow */
                sessionBrowser
              ) : (
              <SongList
                songs={visibleSongs}
                itemsPerPage={itemsPerPage}
                order={listOrder}
                headerLabel={sessionView === 'library'
                  ? undefined
                  : (openSession ? sessionTitle(openSession, t('sessionImportName')) : t('sessionNoneOpen'))}
                emptyLabel={sessionView === 'library'
                  ? undefined
                  : (activeSessionId ? t('sessionEmpty') : t('sessionNoneOpen'))}
                currentSong={currentSong}
                selectedSong={selectedSong}
                likedSongIds={likedSongIds}
                isPlaying={isPlaying}
                /* The list on this page is a queue: the next track follows the
                   previous one, as it always did here. The library's "All songs"
                   tab is not - there "play" means this one track only. */
                onPlay={(song) => playSong(song, visibleSongs)}
                onSelect={(s) => {
                  setSelectedSong(s);
                  setShowRightSidebar(true);
                }}
                onToggleLike={toggleLike}
                onAddToPlaylist={openAddToPlaylistModal}
                onOpenCoverRegen={openCoverRegen}
                onShowDetails={handleShowDetails}
                onDeleteMany={handleDeleteSongs}
                onSongUpdate={handleSongUpdate}
                onCancelJob={cancelGeneration}
                onResetJob={resetSingleJob}
                onCancelAll={cancelAllGenerations}
                onResetAll={resetGeneration}
                activeJobCount={activeJobCount}
              />
              )}

              {/* The agent's message log, opened by the strip's bell */}
              <AgentJournalPanel entries={agentNotices} onRemove={removeNotice} onClearRequest={askClearJournal} />
              {/* The session modals sit at the window's top level (see the
                  end of this file): inside this page they would live in the
                  subtree React hides, and Libraries would never see them. */}
            </div>

            {/* Right Sidebar */}
            {showRightSidebar && selectedSong && (
              <>
              {rightPanel.handle}
              <div
                className="hidden xl:block min-h-0 min-w-0 shrink-0 h-full bg-zinc-50 dark:bg-suno-panel relative z-10 transition-colors duration-300"
                style={{ width: rightPanel.width }}
              >
                <RightSidebar
                  song={selectedSong}
                  onClose={() => setShowRightSidebar(false)}
                  onOpenCoverRegen={() => selectedSong && openCoverRegen(selectedSong)}
                  onReuse={handleReuse}
                  onSongUpdate={handleSongUpdate}
                  isLiked={selectedSong ? likedSongIds.has(selectedSong.id) : false}
                  onToggleLike={toggleLike}
                  onPlay={playSong}
                  isPlaying={isPlaying}
                  currentSong={currentSong}
                />
              </div>
              </>
            )}

            {/* Mobile Toggle Button */}
            <div className="md:hidden absolute top-4 right-4 z-50">
              <button
                onClick={() => setMobileShowList(!mobileShowList)}
                className="bg-zinc-800 text-white px-4 py-2 rounded-full shadow-lg border border-white/10 flex items-center gap-2 text-sm font-bold"
              >
                {mobileShowList ? t('createSong') : t('viewList')}
                <List size={16} />
              </button>
            </div>
          </div>
        );
    }
  };

  // Every song menu and song button takes its actions from here. The value
  // keeps one identity, so the playing clock does not re-render every row.
  const songActionsLatest = useRef<Required<SongActions>>(null as never);
  songActionsLatest.current = {
    reusePrompt: handleReuse,
    replay: handleNativeReplay,
    exportVideo: setSongForVideo,
    addToPlaylist: openAddToPlaylistModal,
    remove: handleDeleteSong,
    update: handleSongUpdate,
  };
  const songActions = useMemo<SongActions>(() => ({
    reusePrompt: song => songActionsLatest.current.reusePrompt(song),
    replay: song => songActionsLatest.current.replay(song),
    exportVideo: song => songActionsLatest.current.exportVideo(song),
    addToPlaylist: song => songActionsLatest.current.addToPlaylist(song),
    remove: song => songActionsLatest.current.remove(song),
    update: song => songActionsLatest.current.update(song),
  }), []);

  return (
    <SongActionsProvider value={songActions}>
    <div className="flex h-dvh min-h-0 min-w-0 flex-col overflow-hidden bg-white dark:bg-suno text-zinc-900 dark:text-white font-sans antialiased selection:bg-pink-500/30 transition-colors duration-300">
      <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden">
        <Sidebar
          currentView={currentView}
          onNavigate={(v) => {
            setCurrentView(v);
            if (v === 'create') {
              setMobileShowList(false);
              window.history.pushState({}, '', '/');
            } else if (v === 'library') {
              window.history.pushState({}, '', '/library');
            } else if (v === 'search') {
              window.history.pushState({}, '', '/search');
            } else if (v === 'news') {
              window.history.pushState({}, '', '/news');
            } else if (v === 'tools') {
              window.history.pushState({}, '', '/tools');
            }
            if (isMobile) setShowLeftSidebar(false);
          }}
          theme={theme}
          onToggleTheme={toggleTheme}
          user={user}
          onOpenSettings={() => setShowSettingsModal(true)}
          isOpen={showLeftSidebar}
          onToggle={() => setShowLeftSidebar(!showLeftSidebar)}
        />

        <main className="relative ml-[72px] flex min-h-0 min-w-0 flex-1 overflow-hidden md:ml-0">
          {createKept && <Activity mode={showingCreate ? 'visible' : 'hidden'}>{renderContent('create')}</Activity>}
          {!(createKept && showingCreate) && renderContent()}
        </main>
      </div>

      {(currentSong || selectedSong) && <Player
        currentSong={currentSong || selectedSong}
        isPlaying={isPlaying}
        onTogglePlay={togglePlay}
        currentTime={currentTime}
        duration={duration}
        onSeek={handleSeek}
        onNext={playNext}
        onPrevious={playPrevious}
        volume={volume}
        onVolumeChange={setVolume}
        playbackRate={playbackRate}
        onPlaybackRateChange={setPlaybackRate}
        audioRef={audioRef}
        isShuffle={isShuffle}
        onToggleShuffle={() => {
          const next = !isShuffle;
          setIsShuffle(next);
          rememberPlaybackMode({ shuffle: next });
        }}
        repeatMode={repeatMode}
        onToggleRepeat={() => setRepeatMode(prev => {
          const next: RepeatMode = prev === 'none' ? 'all' : prev === 'all' ? 'one' : prev === 'one' ? 'stop' : 'none';
          rememberPlaybackMode({ repeatMode: next });
          return next;
        })}
        isLiked={currentSong ? likedSongIds.has(currentSong.id) : false}
        onToggleLike={() => currentSong && toggleLike(currentSong.id)}
        onPlayFirst={playFirst}
      />}

      <PlayerExtras
        queue={playQueue}
        currentSong={currentSong}
        currentTime={currentTime}
        isPlaying={isPlaying}
        volume={volume}
        library={songs}
        onPause={() => setIsPlaying(false)}
        onLeaveWinamp={(exit) => {
          setVolume(exit.volume);
          const song = exit.songId ? songs.find((entry) => entry.id === exit.songId) : null;
          if (song && song.id !== currentSong?.id) {
            // the position waits for the new song to load
            pendingSeekRef.current = exit.seconds;
            playSong(song, playQueue.some((entry) => entry.id === song.id) ? playQueue : undefined);
          } else if (song) {
            handleSeek(exit.seconds);
          }
          // Winamp played a file from disk: the studio's own song stays paused
          setIsPlaying(Boolean(song) && exit.playing);
        }}
        onSavePlaylist={async (songIds) => {
          const playlist = await createNativePlaylist(`Winamp ${new Date().toLocaleString()}`, '', songIds);
          setPlaylists((prev) => [playlist, ...prev]);
          showToast(t('playlistCreated'));
        }}
      />

      <CreatePlaylistModal
        isOpen={isCreatePlaylistModalOpen}
        onClose={() => setIsCreatePlaylistModalOpen(false)}
        onCreate={createPlaylist}
      />
      <AddToPlaylistModal
        isOpen={isAddToPlaylistModalOpen}
        onClose={() => setIsAddToPlaylistModalOpen(false)}
        playlists={playlists}
        onSelect={addSongToPlaylist}
        onCreateNew={() => {
          setIsAddToPlaylistModalOpen(false);
          setIsCreatePlaylistModalOpen(true);
        }}
      />
      <FilesPanel />
      <Toast
        message={toast.message}
        type={toast.type}
        isVisible={toast.isVisible}
        onClose={closeToast}
        duration={toast.type === 'error' ? 8000 : toast.type === 'info' ? 6000 : 3000}
      />
      {/* Cover regen modal — only mounted while a song is selected for regen.
          Unmounting on close revokes blob URLs (see CoverRegenModal cleanup
          effect) so generated previews don't leak across modal opens. */}
      <VideoGeneratorModal
        isOpen={Boolean(songForVideo)}
        song={songForVideo}
        onClose={() => setSongForVideo(null)}
      />
      {songToProcess && (
        <ProcessingModal
          song={songToProcess}
          onClose={() => setSongToProcess(null)}
          onKept={() => { void refreshNativeLibrary(); }}
        />
      )}
      {songForReplay && (
        <ReplayModal
          song={songForReplay}
          clientRef={replayRequestRef}
          onClose={closeReplay}
          onQueued={(jobId) => {
            // A re-render is a generation like any other: it gets its own card
            // with the engine's stages, and lands in the library the same way.
            const tempId = `replay_${jobId}`;
            setSongs(prev => [{
              id: tempId,
              title: songForReplay.title,
              lyrics: songForReplay.lyrics,
              style: songForReplay.style,
              coverUrl: '',
              duration: '--:--',
              createdAt: new Date(),
              isGenerating: true,
              jobId,
              stage: 'stageWaitingInQueue',
              tags: ['yue2'],
            }, ...prev]);
            setIsGenerating(true);
            beginPollingJob(jobId, tempId);
            showToast(t('replayQueued'));
          }}
        />
      )}
      {songForCoverRegen && (
        <CoverRegenModal
          song={songForCoverRegen}
          onClose={() => setSongForCoverRegen(null)}
          onCoverSaved={applyCoverUpdate}
        />
      )}
      <SettingsModal
        onItemsPerPage={setItemsPerPage}
        itemsPerPage={itemsPerPage}
        isOpen={showSettingsModal}
        initialSection={settingsSection}
        onClose={() => { setShowSettingsModal(false); setSettingsSection(null); }}
        theme={theme}
        onToggleTheme={toggleTheme}
      />

      {/* Mobile Details Modal */}
      {showMobileDetails && selectedSong && (
        <div className="fixed inset-0 z-60 flex justify-end xl:hidden">
          <div
            className="absolute inset-0 bg-black/60 backdrop-blur-xs animate-in fade-in"
            onClick={() => setShowMobileDetails(false)}
          />
          <div className="relative w-full max-w-md h-full bg-zinc-50 dark:bg-suno-panel shadow-2xl animate-in slide-in-from-right duration-300 border-l border-white/10">
            <RightSidebar
              song={selectedSong}
              onClose={() => setShowMobileDetails(false)}
              onOpenCoverRegen={() => selectedSong && openCoverRegen(selectedSong)}
              onReuse={handleReuse}
              onSongUpdate={handleSongUpdate}
              isLiked={selectedSong ? likedSongIds.has(selectedSong.id) : false}
              onToggleLike={toggleLike}
              onPlay={playSong}
              isPlaying={isPlaying}
              currentSong={currentSong}
            />
          </div>
        </div>
      )}

      {/* Creating a session: the service names and keeps it, the new one opens at once */}
      <SessionCreateModal
        isOpen={isSessionCreateOpen}
        onCreate={(rawName) => {
          void createWorkspace(uniqueSessionName(rawName, sessions))
            .then(async (created) => {
              await openWorkspace(created.id);
              await readSessions();
              setActiveSessionId(created.id);
              setSessionView('session');
              setCenterView('tracks');
              // Whatever the person asked for before the session existed now has a home.
              const pending = pendingGenerateRef.current;
              pendingGenerateRef.current = null;
              if (pending) void handleGenerate(pending);
            })
            .catch(() => undefined);
          setIsSessionCreateOpen(false);
        }}
        onDismiss={() => setIsSessionCreateOpen(false)}
      />

      {/* A session opened from the library page: ask whether to walk over to it */}
      <EditModeModal
        offer={editModeOffer}
        onAccept={() => {
          setEditModeOffer(null);
          setSessionView('session');
          setCenterView('tracks');
          setCurrentView('create');
        }}
        onDecline={() => setEditModeOffer(null)}
      />

      {/* The agent asking to do something that cannot be undone */}
      <AgentConfirmModal request={agentPermission} />
      <SessionChoiceModal request={sessionChoice} />

      {/* The agent's request: answered once, always, elsewhere, or not at all.
          A modal belongs here with the author's own modals - only the window's
          top level is on screen whatever page is - the pages come and go. */}
      <SessionConfirmModal
        request={agentRequest}
        sessions={sessions}
        onAllowOnce={() => agentRequest?.answer('once')}
        onAllowSession={() => agentRequest?.answer('session')}
        onAllowAlways={() => agentRequest?.answer('always')}
        onAskEveryTime={() => agentRequest?.answer('ask')}
        onDenyAlways={() => agentRequest?.answer('deny_always')}
        onOpenExisting={(sessionId) => {
          setSessionView('session');
          setCenterView('tracks');
          void openWorkspace(sessionId)
            .then(() => readSessions())
            .then(() => agentRequest?.answer('once'))
            .catch(() => agentRequest?.answer('denied'));
        }}
        onCreate={(name) => {
          void createWorkspace(name)
            .then((created) => openWorkspace(created.id))
            .then(() => readSessions())
            .then(() => agentRequest?.answer('once'))
            .catch(() => agentRequest?.answer('denied'));
        }}
        onDecline={() => agentRequest?.answer('denied')}
      />

      <ConfirmDialog
        isOpen={confirmDialog !== null}
        title={confirmDialog?.title ?? ''}
        message={confirmDialog?.message ?? ''}
        confirmLabel={confirmDialog?.confirmLabel}
        onConfirm={() => confirmDialog?.onConfirm()}
        onCancel={() => setConfirmDialog(null)}
      />
    </div>
    </SongActionsProvider>
  );
}

export default function App() {
  return (
    <I18nProvider>
      <AppContent />
    </I18nProvider>
  );
}
