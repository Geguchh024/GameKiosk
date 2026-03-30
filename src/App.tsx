import { useState, useEffect, useCallback, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useGamepad } from "./useGamepad";
import { fetchPopularGames, searchGames, searchGameImages, validateCredentials, type IgdbGame, type IgdbImage } from "./igdb";
import { getCachedImage, cacheImage } from "./imageCache";
import type { Program, TabFilter, NavItem } from "./types";

type RunningGame = { program_id: string; program_name: string; pid: number; exe_path: string; active: boolean; muted: boolean };
type AppSettings = {
  igdb_client_id: string;
  igdb_client_secret: string;
  tray_mouse_speed?: number;
  tray_mouse_enabled?: boolean;
};

const TRAY_MOUSE_SPEED_MIN = 0.2;
const TRAY_MOUSE_SPEED_MAX = 1.5;
const TRAY_MOUSE_SPEED_STEP = 0.05;

function clampTrayMouseSpeed(speed: number): number {
  return Math.min(TRAY_MOUSE_SPEED_MAX, Math.max(TRAY_MOUSE_SPEED_MIN, Number(speed.toFixed(2))));
}

/** Map Rust snake_case fields to frontend camelCase */
function normalizePrograms(raw: Program[]): Program[] {
  return raw.map((p) => ({
    ...p,
    coverUrl: p.cover_url ?? p.coverUrl,
    favorite: p.favorite ?? false,
  }));
}

function formatFileSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(i > 0 ? 1 : 0)} ${units[i]}`;
}

/* ── Focus zones for controller navigation ── */
type FocusZone = "sidebar" | "running" | "tabs" | "grid";
type OnboardingStep = {
  title: string;
  body: string;
  points: string[];
};

const COLUMNS = 4;
const SPLASH_DURATION_MS = 1600;
const ONBOARDING_STORAGE_KEY = "gamekiosk.onboardingComplete";
const CONTROLLER_WARNING_STORAGE_KEY = "gamekiosk.controllerKeyWarningDismissed";

const COVER_GRADIENTS = [
  "linear-gradient(135deg, #1a1a2e, #2d2d44)",
  "linear-gradient(135deg, #1e1e30, #2a2a42)",
  "linear-gradient(135deg, #1c1c2c, #28283e)",
  "linear-gradient(135deg, #201e2e, #302e42)",
  "linear-gradient(135deg, #1a1c2a, #2c2e40)",
  "linear-gradient(135deg, #1e1a28, #302c3e)",
  "linear-gradient(135deg, #1c1e2c, #2e3040)",
];

type Theme = "obsidian" | "frost" | "dusk";

const THEMES: { id: Theme; label: string; swatch: string }[] = [
  { id: "obsidian", label: "Obsidian", swatch: "#101014" },
  { id: "frost", label: "Frost", swatch: "#f4f4f8" },
  { id: "dusk", label: "Dusk", swatch: "#1c1a22" },
];

const NAV_ITEMS: { id: NavItem; icon: string; label: string }[] = [
  { id: "library", icon: "⊞", label: "Library" },
  { id: "downloads", icon: "↓", label: "Downloads" },
  { id: "store", icon: "🛒", label: "Store" },
  { id: "settings", icon: "⚙", label: "Settings" },
];

const TABS: { id: TabFilter; label: string }[] = [
  { id: "all", label: "All Games" },
  { id: "installed", label: "Installed" },
  { id: "favorites", label: "Favorites" },
  { id: "recent", label: "Recent" },
];

const ONBOARDING_STEPS: OnboardingStep[] = [
  {
    title: "Import your library",
    body: "GameKiosk keeps your games in one place so you can launch fast without leaving fullscreen mode.",
    points: [
      "Use Add Game to browse for .exe, .lnk, .bat, .cmd, .url, or appref-ms files.",
      "Right-click a game or press E to edit its name and cover art.",
    ],
  },
  {
    title: "Move with keyboard or controller",
    body: "The UI is built for both mouse and couch navigation, with quick shortcuts for the most common actions.",
    points: [
      "Keyboard: / search, Tab switch zones, F favorite, Del remove, Enter launch.",
      "Controller: LB/RB switch sections, LT/RT switch tabs, A launch, B back.",
    ],
  },
  {
    title: "Tune the look and metadata",
    body: "Personalize the theme and connect IGDB when you want richer cover art and the store browser.",
    points: [
      "Open Settings to change the theme or turn controller mode on and off.",
      "Set up IGDB to fetch cover art and browse popular games from the store tab.",
    ],
  },
];

function App() {
  const [programs, setPrograms] = useState<Program[]>([]);
  const [showSplash, setShowSplash] = useState(true);
  const [showOnboarding, setShowOnboarding] = useState(
    () => localStorage.getItem(ONBOARDING_STORAGE_KEY) !== "true"
  );
  const [onboardingStep, setOnboardingStep] = useState(0);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [launching, setLaunching] = useState(false);
  const [message, setMessage] = useState("");
  const [activeTab, setActiveTab] = useState<TabFilter>("all");
  const [activeNav, setActiveNav] = useState<NavItem>("library");
  const [searchQuery, setSearchQuery] = useState("");
  const [storeGames, setStoreGames] = useState<IgdbGame[]>([]);
  const [storeLoading, setStoreLoading] = useState(false);
  const [storeSearch, setStoreSearch] = useState("");
  const [storePage, setStorePage] = useState(0);
  const [storeHasMore, setStoreHasMore] = useState(true);
  const STORE_PAGE_SIZE = 20;
  const [controllerMode, setControllerMode] = useState(() => localStorage.getItem("controllerMode") === "true");
  const [theme, setTheme] = useState<Theme>(() => (localStorage.getItem("theme") as Theme) || "obsidian");
  const [trayMouseSpeed, setTrayMouseSpeed] = useState(0.6);
  const [trayMouseEnabled, setTrayMouseEnabled] = useState(true);
  const [focusZone, setFocusZone] = useState<FocusZone>("grid");
  const [sidebarIndex, setSidebarIndex] = useState(0);
  const [tabIndex, setTabIndex] = useState(0);
  const [showControllerWarning, setShowControllerWarning] = useState(false);

  // Settings controller navigation
  const [settingsIndex, setSettingsIndex] = useState(0);
  const [appActionIndex, setAppActionIndex] = useState(0);
  const SETTINGS_ITEMS = 6; // Theme, Controller Mode, Tray Cursor Speed, IGDB API, Library Stats, Application
  const SETTINGS_APP_ACTIONS = 2; // Minimize to Tray, Quit

  // Downloads state
  const [downloadEntries, setDownloadEntries] = useState<{ name: string; path: string; is_dir: boolean; size: number; modified: number }[]>([]);
  const [downloadsLoading, setDownloadsLoading] = useState(false);
  const [downloadsIndex, setDownloadsIndex] = useState(0);

  // Running games state
  const [runningGames, setRunningGames] = useState<{ program_id: string; program_name: string; pid: number; exe_path: string; active: boolean; muted: boolean }[]>([]);
  const [runningActionIndex, setRunningActionIndex] = useState(0);

  // Edit modal state
  const [editingProgram, setEditingProgram] = useState<Program | null>(null);
  const [editName, setEditName] = useState("");
  const [editImageUrl, setEditImageUrl] = useState("");
  const [editFocusIndex, setEditFocusIndex] = useState(0); // for controller in modal

  // Image picker popup state
  const [showImagePicker, setShowImagePicker] = useState(false);
  const [imagePickerQuery, setImagePickerQuery] = useState("");
  const [imagePickerResults, setImagePickerResults] = useState<IgdbImage[]>([]);
  const [imagePickerLoading, setImagePickerLoading] = useState(false);
  const [imagePickerProgram, setImagePickerProgram] = useState<Program | null>(null);

  // Legacy image search in edit modal (kept for backward compat)
  const [imageSearchQuery, setImageSearchQuery] = useState("");
  const [imageSearchResults, setImageSearchResults] = useState<IgdbGame[]>([]);
  const [imageSearching, setImageSearching] = useState(false);

  // IGDB credentials
  const [igdbClientId, setIgdbClientId] = useState("");
  const [igdbClientSecret, setIgdbClientSecret] = useState("");
  const [igdbConfigured, setIgdbConfigured] = useState(false);
  const [showApiSetup, setShowApiSetup] = useState(false);
  const [setupClientId, setSetupClientId] = useState("");
  const [setupClientSecret, setSetupClientSecret] = useState("");
  const [setupValidating, setSetupValidating] = useState(false);
  const [setupError, setSetupError] = useState("");

  const gridRef = useRef<HTMLDivElement>(null);
  const cardRefs = useRef<Map<number, HTMLButtonElement>>(new Map());
  const searchInputRef = useRef<HTMLInputElement>(null);
  const toastTimerRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  const onboardingRef = useRef<HTMLDivElement>(null);

  const appInteractive = !showSplash && !showOnboarding;

  // Persist controller mode
  useEffect(() => { localStorage.setItem("controllerMode", String(controllerMode)); }, [controllerMode]);

  // Persist and apply theme
  useEffect(() => {
    localStorage.setItem("theme", theme);
    document.documentElement.setAttribute("data-theme", theme);
  }, [theme]);

  useEffect(() => {
    invoke<Program[]>("get_programs").then((raw) => setPrograms(normalizePrograms(raw))).catch(console.error);
  }, []);

  useEffect(() => {
    const timer = setTimeout(() => setShowSplash(false), SPLASH_DURATION_MS);
    return () => clearTimeout(timer);
  }, []);

  useEffect(() => {
    if (!showSplash && showOnboarding) {
      onboardingRef.current?.focus();
    }
  }, [showSplash, showOnboarding]);

  useEffect(() => {
    if (!showSplash && !showOnboarding && controllerMode && localStorage.getItem(CONTROLLER_WARNING_STORAGE_KEY) !== "true") {
      setShowControllerWarning(true);
    }
    if (!controllerMode) {
      setShowControllerWarning(false);
    }
  }, [controllerMode, showSplash, showOnboarding]);

  // Load IGDB credentials from backend
  useEffect(() => {
    invoke<AppSettings>("get_settings").then((s) => {
      if (s.igdb_client_id && s.igdb_client_secret) {
        setIgdbClientId(s.igdb_client_id);
        setIgdbClientSecret(s.igdb_client_secret);
        setIgdbConfigured(true);
      }
      if (typeof s.tray_mouse_speed === "number") {
        setTrayMouseSpeed(clampTrayMouseSpeed(s.tray_mouse_speed));
      }
      if (typeof s.tray_mouse_enabled === "boolean") {
        setTrayMouseEnabled(s.tray_mouse_enabled);
      }
    }).catch(console.error);
  }, []);

  // Running games: refresh + listen for changes
  const refreshRunningGames = useCallback(() => {
    invoke<RunningGame[]>("get_running_games").then(setRunningGames).catch(console.error);
  }, []);

  useEffect(() => {
    refreshRunningGames();
    // Poll every 5 seconds to detect closed games
    const interval = setInterval(refreshRunningGames, 5000);
    // Listen for backend events
    const unlisten = listen("running-games-changed", () => refreshRunningGames());
    return () => { clearInterval(interval); unlisten.then((fn) => fn()); };
  }, [refreshRunningGames]);

  // Whenever backend brings launcher to front, reset controller focus to the grid.
  useEffect(() => {
    const unlisten = listen("launcher-shown", () => {
      setFocusZone("grid");
      setRunningActionIndex(0);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  useEffect(() => {
    const unlisten = listen<boolean>("tray-mouse-enabled-changed", (event) => {
      const enabled = !!event.payload;
      setTrayMouseEnabled(enabled);
      showMessage(`Tray cursor ${enabled ? "enabled" : "disabled"}`);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  useEffect(() => {
    if (activeNav === "store" && storeGames.length === 0 && igdbConfigured) {
      setStoreLoading(true);
      setStorePage(0);
      fetchPopularGames(STORE_PAGE_SIZE, 0).then((games) => {
        setStoreGames(games);
        setStoreHasMore(games.length >= STORE_PAGE_SIZE);
        setStoreLoading(false);
      });
    }
  }, [activeNav, igdbConfigured]);

  // Load downloads folder contents
  useEffect(() => {
    if (activeNav === "downloads") {
      setDownloadsLoading(true);
      invoke<{ name: string; path: string; is_dir: boolean; size: number; modified: number }[]>("list_downloads")
        .then((entries) => { setDownloadEntries(entries); setDownloadsLoading(false); setDownloadsIndex(0); })
        .catch(() => { setDownloadEntries([]); setDownloadsLoading(false); });
    }
  }, [activeNav]);

  useEffect(() => {
    if (activeNav !== "store" || !storeSearch.trim()) {
      if (activeNav === "store" && !storeSearch.trim() && storeGames.length === 0 && igdbConfigured) {
        fetchPopularGames(STORE_PAGE_SIZE, 0).then((games) => {
          setStoreGames(games);
          setStoreHasMore(games.length >= STORE_PAGE_SIZE);
          setStorePage(0);
        });
      }
      return;
    }
    if (!igdbConfigured) return;
    const timer = setTimeout(() => {
      setStoreLoading(true);
      setStorePage(0);
      searchGames(storeSearch).then((games) => {
        setStoreGames(games);
        setStoreHasMore(false); // search doesn't paginate
        setStoreLoading(false);
      });
    }, 400);
    return () => clearTimeout(timer);
  }, [storeSearch, activeNav, igdbConfigured]);

  // Resolved image cache: maps program id -> usable image src (data URL or remote URL)
  const [resolvedCovers, setResolvedCovers] = useState<Record<string, string>>({});

  // On mount, hydrate resolved covers from localStorage cache for instant display
  useEffect(() => {
    const initial: Record<string, string> = {};
    programs.forEach((p) => {
      const url = p.coverUrl || p.cover_url;
      if (url) {
        const cached = getCachedImage(url);
        if (cached) initial[p.id] = cached;
      }
    });
    if (Object.keys(initial).length > 0) {
      setResolvedCovers((prev) => ({ ...prev, ...initial }));
    }
  }, [programs.length]); // only re-run when program count changes

  // Auto-fetch cover images for programs missing them, cache locally, and persist URL to backend
  const fetchedRef = useRef<Set<string>>(new Set());
  // Track programs where the user manually set a custom image — never auto-fetch for these
  const userSetCoversRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    programs.forEach((p) => {
      if (fetchedRef.current.has(p.id)) return;
      if (userSetCoversRef.current.has(p.id)) return;
      const url = p.coverUrl || p.cover_url;

      if (url) {
        // URL exists but may not be cached yet — cache it in background
        if (!resolvedCovers[p.id]) {
          fetchedRef.current.add(p.id);
          cacheImage(url).then((src) => {
            setResolvedCovers((prev) => ({ ...prev, [p.id]: src }));
          });
        }
        return;
      }

      // No URL at all — search IGDB, persist, and cache
      if (!igdbConfigured) return;
      fetchedRef.current.add(p.id);
      searchGames(p.name).then(async (results) => {
        if (results[0]?.background_image) {
          const remoteUrl = results[0].background_image!;
          // Cache the image locally
          const cachedSrc = await cacheImage(remoteUrl);
          setResolvedCovers((prev) => ({ ...prev, [p.id]: cachedSrc }));
          // Persist URL to backend
          invoke<Program[]>("update_program", { id: p.id, coverUrl: remoteUrl })
            .then((raw) => setPrograms(normalizePrograms(raw)))
            .catch(() => {
              setPrograms((prev) => prev.map((prog) =>
                prog.id === p.id ? { ...prog, coverUrl: remoteUrl, cover_url: remoteUrl } : prog
              ));
            });
        }
      });
    });
  }, [programs]);

  const showMessage = (msg: string) => {
    setMessage(msg);
    if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
    toastTimerRef.current = setTimeout(() => setMessage(""), 3000);
  };

  const dismissControllerWarning = useCallback(() => {
    localStorage.setItem(CONTROLLER_WARNING_STORAGE_KEY, "true");
    setShowControllerWarning(false);
  }, []);

  const handleSwitchToGame = useCallback(async (programId: string) => {
    try {
      const updated = await invoke<RunningGame[]>("switch_to_game", { programId });
      setRunningGames(updated);
    } catch (e: any) { showMessage(`Error switching: ${e}`); }
  }, []);

  const handleCloseGame = useCallback(async (programId: string) => {
    const updated = await invoke<RunningGame[]>("close_game", { programId });
    setRunningGames(updated);
    showMessage("Game closed");
  }, []);

  const filteredPrograms = useMemo(() => {
    let filtered = programs;
    if (searchQuery) filtered = filtered.filter((p) => p.name.toLowerCase().includes(searchQuery.toLowerCase()));
    switch (activeTab) {
      case "installed": return filtered.filter((p) => p.installed !== false);
      case "favorites": return filtered.filter((p) => p.favorite);
      case "recent": return [...filtered].sort((a, b) => (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0));
      default: return filtered;
    }
  }, [programs, activeTab, searchQuery]);

  const totalItems = activeNav === "store" ? storeGames.length + (storeHasMore && !storeSearch.trim() ? 1 : 0) : filteredPrograms.length + 1;
  const runningActionCount = runningGames.length * 2;
  const clamp = (val: number, max: number) => Math.max(0, Math.min(val, max - 1));

  useEffect(() => {
    if (runningActionCount === 0) {
      setRunningActionIndex(0);
      if (focusZone === "running") {
        setFocusZone(activeNav === "library" ? "tabs" : "grid");
      }
      return;
    }
    setRunningActionIndex((i) => clamp(i, runningActionCount));
  }, [runningActionCount, focusZone, activeNav]);

  useEffect(() => {
    requestAnimationFrame(() => {
      let el: Element | null = cardRefs.current.get(selectedIndex) ?? null;
      if (!el) {
        el = document.querySelector(".game-card.selected, .downloads-item.zone-focused, .store-page-btn.selected, .running-chip-btn.zone-focused");
      }
      if (el) {
        el.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "smooth" });
      }
    });
  }, [selectedIndex, downloadsIndex, settingsIndex, runningActionIndex, activeNav]);

  /* ── Handlers ── */
  const handleConfirm = useCallback(async () => {
    if (showControllerWarning) return;
    if (selectedIndex === filteredPrograms.length) { await handleAdd(); return; }
    const program = filteredPrograms[selectedIndex];
    if (program && !launching) {
      setLaunching(true);
      showMessage(`Launching ${program.name}...`);
      try {
        await invoke("launch_program", { path: program.path, programId: program.id, programName: program.name });
      } catch (e: any) { showMessage(`Error: ${e}`); }
      setTimeout(() => { setLaunching(false); refreshRunningGames(); }, 3000);
    }
  }, [selectedIndex, filteredPrograms, launching, refreshRunningGames, showControllerWarning]);

  const handleAdd = useCallback(async () => {
    if (showControllerWarning) return;
    try {
      const selected = await open({ multiple: false, filters: [{ name: "Programs", extensions: ["exe", "lnk", "bat", "cmd", "url", "appref-ms"] }, { name: "All Files", extensions: ["*"] }] });
      if (selected) {
        const filePath = String(selected);
        const name = filePath.split("\\").pop()?.replace(/\.[^.]+$/, "") ?? "Program";
        const updated = await invoke<Program[]>("add_program", { name, path: filePath });
        setPrograms(normalizePrograms(updated));
        showMessage(`Added ${name}`);
      }
    } catch (e: any) { showMessage(`Error adding program: ${e}`); }
  }, [showControllerWarning]);

  const handleDelete = useCallback(async () => {
    if (showControllerWarning) return;
    if (selectedIndex < filteredPrograms.length) {
      const program = filteredPrograms[selectedIndex];
      const updated = await invoke<Program[]>("remove_program", { id: program.id });
      setPrograms(normalizePrograms(updated));
      setSelectedIndex((i) => clamp(i, updated.length + 1));
      showMessage(`Removed ${program.name}`);
    }
  }, [selectedIndex, filteredPrograms, showControllerWarning]);

  const toggleFavorite = useCallback(() => {
    if (showControllerWarning) return;
    if (selectedIndex < filteredPrograms.length) {
      const program = filteredPrograms[selectedIndex];
      const isFav = !program.favorite;
      invoke<Program[]>("update_program", { id: program.id, favorite: isFav }).then((raw) => {
        setPrograms(normalizePrograms(raw));
      }).catch(() => {
        setPrograms((prev) => prev.map((p) => (p.id === program.id ? { ...p, favorite: isFav } : p)));
      });
      showMessage(isFav ? `Added ${program.name} to favorites` : `Removed from favorites`);
    }
  }, [selectedIndex, filteredPrograms, showControllerWarning]);

  const openEditModal = useCallback((program: Program) => {
    setEditingProgram(program);
    setEditName(program.name);
    setEditImageUrl(program.coverUrl ?? program.cover_url ?? "");
    setImageSearchQuery("");
    setImageSearchResults([]);
    setEditFocusIndex(0);
  }, []);

  const closeEditModal = useCallback(() => {
    setEditingProgram(null);
    setEditName("");
    setEditImageUrl("");
    setImageSearchQuery("");
    setImageSearchResults([]);
  }, []);

  const saveEdit = useCallback(async () => {
    if (!editingProgram || !editName.trim()) return;
    const trimmedName = editName.trim();
    const url = editImageUrl.trim() || null;
    const programId = editingProgram.id;
    const existingUrl = editingProgram.coverUrl ?? editingProgram.cover_url ?? null;
    // Use the edited URL if changed, otherwise keep the existing one
    const finalUrl = url !== null ? url : existingUrl ?? null;
    closeEditModal();
    // Mark this program so auto-fetch never overwrites the user's choice
    if (finalUrl) {
      userSetCoversRef.current.add(programId);
      // Also reset fetchedRef so the effect doesn't skip caching the new URL
      fetchedRef.current.delete(programId);
    }
    // Optimistic local update so the UI reflects the change immediately
    setPrograms((prev) => prev.map((p) =>
      p.id === programId ? { ...p, name: trimmedName, coverUrl: finalUrl ?? undefined, cover_url: finalUrl } : p
    ));
    // Cache the new image if provided
    if (finalUrl) {
      cacheImage(finalUrl).then((src) => {
        setResolvedCovers((prev) => ({ ...prev, [programId]: src }));
      });
    }
    try {
      const raw = await invoke<Program[]>("update_program", {
        id: programId,
        name: trimmedName,
        coverUrl: finalUrl ?? "",
      });
      setPrograms(normalizePrograms(raw));
      showMessage(`Updated ${trimmedName}`);
    } catch (e) {
      console.error("Failed to save program:", e);
      showMessage(`Updated ${trimmedName} (offline)`);
    }
  }, [editingProgram, editName, editImageUrl, closeEditModal]);

  const handleImageSearch = useCallback(() => {
    if (!imageSearchQuery.trim() || !igdbConfigured) return;
    setImageSearching(true);
    searchGames(imageSearchQuery.trim()).then((results) => { setImageSearchResults(results); setImageSearching(false); });
  }, [imageSearchQuery, igdbConfigured]);

  // Image picker popup
  const openImagePicker = useCallback((program: Program) => {
    setImagePickerProgram(program);
    setImagePickerQuery(program.name);
    setImagePickerResults([]);
    setShowImagePicker(true);
    // Auto-search on open
    setImagePickerLoading(true);
    searchGameImages(program.name).then((results) => {
      setImagePickerResults(results);
      setImagePickerLoading(false);
    });
  }, []);

  const closeImagePicker = useCallback(() => {
    setShowImagePicker(false);
    setImagePickerProgram(null);
    setImagePickerResults([]);
    setImagePickerQuery("");
  }, []);

  const handleImagePickerSearch = useCallback(() => {
    if (!imagePickerQuery.trim() || !igdbConfigured) return;
    setImagePickerLoading(true);
    searchGameImages(imagePickerQuery.trim()).then((results) => {
      setImagePickerResults(results);
      setImagePickerLoading(false);
    });
  }, [imagePickerQuery, igdbConfigured]);

  const selectPickerImage = useCallback(async (url: string) => {
    if (!imagePickerProgram) return;
    const programId = imagePickerProgram.id;
    const programName = imagePickerProgram.name;
    closeImagePicker();
    closeEditModal();

    // Mark as user-set so auto-fetch never overwrites
    userSetCoversRef.current.add(programId);
    fetchedRef.current.delete(programId);

    // Optimistic update
    setPrograms((prev) => prev.map((p) =>
      p.id === programId ? { ...p, coverUrl: url, cover_url: url } : p
    ));
    // Cache the image
    cacheImage(url).then((src) => {
      setResolvedCovers((prev) => ({ ...prev, [programId]: src }));
    });
    // Persist to backend
    try {
      const raw = await invoke<Program[]>("update_program", { id: programId, coverUrl: url });
      setPrograms(normalizePrograms(raw));
      showMessage(`Updated cover for ${programName}`);
    } catch {
      showMessage(`Updated cover for ${programName} (offline)`);
    }
  }, [imagePickerProgram, closeImagePicker]);

  // Store: add game to library — opens IGDB page to find/download
  const addStoreGameToLibrary = useCallback(async (game: IgdbGame) => {
    try {
      const igdbUrl = `https://www.igdb.com/games/${game.name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/-+$/, "")}`;
      await invoke("open_url_in_browser", { url: igdbUrl });
      showMessage(`Opening page for ${game.name}`);
    } catch (e: any) {
      showMessage(`Error: ${e}`);
    }
  }, []);

  // Store: load next page
  const loadStoreNextPage = useCallback(() => {
    if (storeLoading || !storeHasMore || storeSearch.trim()) return;
    const nextPage = storePage + 1;
    setStoreLoading(true);
    fetchPopularGames(STORE_PAGE_SIZE, nextPage * STORE_PAGE_SIZE).then((games) => {
      setStoreGames((prev) => [...prev, ...games]);
      setStoreHasMore(games.length >= STORE_PAGE_SIZE);
      setStorePage(nextPage);
      setStoreLoading(false);
    });
  }, [storeLoading, storeHasMore, storePage, storeSearch]);

  // Store: load previous page (reset to page 0)
  const loadStorePrevPage = useCallback(() => {
    if (storeLoading || storePage === 0) return;
    setStoreLoading(true);
    setStorePage(0);
    fetchPopularGames(STORE_PAGE_SIZE, 0).then((games) => {
      setStoreGames(games);
      setStoreHasMore(games.length >= STORE_PAGE_SIZE);
      setStoreLoading(false);
      setSelectedIndex(0);
    });
  }, [storeLoading, storePage]);

  const handleSaveApiCredentials = useCallback(async () => {
    if (!setupClientId.trim() || !setupClientSecret.trim()) return;
    setSetupValidating(true);
    setSetupError("");
    const valid = await validateCredentials(setupClientId.trim(), setupClientSecret.trim());
    if (!valid) {
      setSetupError("Invalid credentials. Check your Client ID and Secret.");
      setSetupValidating(false);
      return;
    }
    try {
      await invoke("save_igdb_credentials", {
        clientId: setupClientId.trim(),
        clientSecret: setupClientSecret.trim(),
      });
    } catch (e) {
      console.error("Failed to save credentials:", e);
    }
    setIgdbClientId(setupClientId.trim());
    setIgdbClientSecret(setupClientSecret.trim());
    setIgdbConfigured(true);
    setSetupValidating(false);
    setShowApiSetup(false);
    showMessage("IGDB API connected");
  }, [setupClientId, setupClientSecret]);

  const persistTrayMouseSpeed = useCallback((nextSpeed: number) => {
    invoke<AppSettings>("save_tray_mouse_settings", { speed: nextSpeed })
      .then((s) => {
        if (typeof s.tray_mouse_speed === "number") {
          setTrayMouseSpeed(clampTrayMouseSpeed(s.tray_mouse_speed));
        }
        if (typeof s.tray_mouse_enabled === "boolean") {
          setTrayMouseEnabled(s.tray_mouse_enabled);
        }
      })
      .catch((e) => {
        console.error("Failed to save tray mouse speed:", e);
      });
  }, []);

  const adjustTrayMouseSpeed = useCallback((delta: number) => {
    setTrayMouseSpeed((prev) => {
      const next = clampTrayMouseSpeed(prev + delta);
      if (next !== prev) {
        persistTrayMouseSpeed(next);
      }
      return next;
    });
  }, [persistTrayMouseSpeed]);

  /* ── Zone-aware navigation ── */
  // Edit modal items: 0 = name input, 1 = change cover btn (if igdb), 2 = cancel, 3 = save
  const EDIT_MODAL_ITEMS = igdbConfigured ? 4 : 3;

  const navigateUp = useCallback(() => {
    if (showControllerWarning) return;
    if (editingProgram) {
      setEditFocusIndex((i) => Math.max(0, i - 1));
      return;
    }
    if (focusZone === "running") {
      return;
    }
    if (activeNav === "settings") {
      setSettingsIndex((i) => Math.max(0, i - 1));
    } else if (activeNav === "downloads") {
      setDownloadsIndex((i) => Math.max(0, i - 1));
    } else if (focusZone === "grid") {
      setSelectedIndex((i) => {
        if (i < COLUMNS) {
          if (activeNav === "library") setFocusZone("tabs");
          else if (runningGames.length > 0) setFocusZone("running");
          return i;
        }
        return clamp(i - COLUMNS, totalItems);
      });
    } else if (focusZone === "tabs") {
      if (runningGames.length > 0) setFocusZone("running");
    } else if (focusZone === "sidebar") {
      setSidebarIndex((i) => clamp(i - 1, NAV_ITEMS.length));
    }
  }, [editingProgram, activeNav, focusZone, totalItems, runningGames.length, showControllerWarning]);

  const navigateDown = useCallback(() => {
    if (showControllerWarning) return;
    if (editingProgram) {
      setEditFocusIndex((i) => Math.min(EDIT_MODAL_ITEMS - 1, i + 1));
      return;
    }
    if (focusZone === "running") {
      setFocusZone(activeNav === "library" ? "tabs" : "sidebar");
      return;
    }
    if (activeNav === "settings") {
      setSettingsIndex((i) => Math.min(SETTINGS_ITEMS - 1, i + 1));
    } else if (activeNav === "downloads") {
      setDownloadsIndex((i) => Math.min(downloadEntries.length - 1, i + 1));
    } else if (focusZone === "tabs") {
      setFocusZone("grid");
    } else if (focusZone === "grid") {
      setSelectedIndex((i) => clamp(i + COLUMNS, totalItems));
    } else if (focusZone === "sidebar") {
      setSidebarIndex((i) => clamp(i + 1, NAV_ITEMS.length));
    }
  }, [editingProgram, activeNav, focusZone, totalItems, downloadEntries.length, showControllerWarning]);

  const navigateLeft = useCallback(() => {
    if (showControllerWarning) return;
    if (focusZone === "running") {
      if (runningActionCount > 0) setRunningActionIndex((i) => clamp(i - 1, runningActionCount));
      return;
    }
    if (activeNav === "settings") {
      // Cycle theme left
      if (settingsIndex === 0) {
        const idx = THEMES.findIndex((t) => t.id === theme);
        setTheme(THEMES[(idx - 1 + THEMES.length) % THEMES.length].id);
      } else if (settingsIndex === 2) {
        adjustTrayMouseSpeed(-TRAY_MOUSE_SPEED_STEP);
      } else if (settingsIndex === 5) {
        setAppActionIndex((i) => (i - 1 + SETTINGS_APP_ACTIONS) % SETTINGS_APP_ACTIONS);
      }
    } else if (focusZone === "grid") {
      setSelectedIndex((i) => clamp(i - 1, totalItems));
    } else if (focusZone === "tabs") {
      setTabIndex((i) => clamp(i - 1, TABS.length));
    }
  }, [activeNav, settingsIndex, theme, focusZone, totalItems, runningActionCount, adjustTrayMouseSpeed, showControllerWarning]);

  const navigateRight = useCallback(() => {
    if (showControllerWarning) return;
    if (focusZone === "running") {
      if (runningActionCount > 0) setRunningActionIndex((i) => clamp(i + 1, runningActionCount));
      return;
    }
    if (activeNav === "settings") {
      // Cycle theme right
      if (settingsIndex === 0) {
        const idx = THEMES.findIndex((t) => t.id === theme);
        setTheme(THEMES[(idx + 1) % THEMES.length].id);
      } else if (settingsIndex === 2) {
        adjustTrayMouseSpeed(TRAY_MOUSE_SPEED_STEP);
      } else if (settingsIndex === 5) {
        setAppActionIndex((i) => (i + 1) % SETTINGS_APP_ACTIONS);
      }
    } else if (focusZone === "grid") {
      setSelectedIndex((i) => clamp(i + 1, totalItems));
    } else if (focusZone === "tabs") {
      setTabIndex((i) => clamp(i + 1, TABS.length));
    }
  }, [activeNav, settingsIndex, theme, focusZone, totalItems, runningActionCount, adjustTrayMouseSpeed, showControllerWarning]);

  const zoneConfirm = useCallback(() => {
    if (showControllerWarning) {
      dismissControllerWarning();
      return;
    }
    if (focusZone === "running") {
      const gameIndex = Math.floor(runningActionIndex / 2);
      const actionIndex = runningActionIndex % 2; // 0 = switch/resume, 1 = close
      const rg = runningGames[gameIndex];
      if (!rg) return;
      if (actionIndex === 0) handleSwitchToGame(rg.program_id);
      else handleCloseGame(rg.program_id);
      return;
    }
    if (activeNav === "settings") {
      if (settingsIndex === 0) {
        // Cycle theme forward on A press
        const idx = THEMES.findIndex((t) => t.id === theme);
        setTheme(THEMES[(idx + 1) % THEMES.length].id);
      } else if (settingsIndex === 1) {
        setControllerMode((v) => !v);
      } else if (settingsIndex === 2) {
        adjustTrayMouseSpeed(TRAY_MOUSE_SPEED_STEP);
      } else if (settingsIndex === 3) {
        setSetupClientId(igdbClientId);
        setSetupClientSecret(igdbClientSecret);
        setSetupError("");
        setShowApiSetup(true);
      } else if (settingsIndex === 5) {
        if (appActionIndex === 0) {
          invoke("hide_launcher").catch(console.error);
        } else {
          invoke("quit_app").catch(console.error);
        }
      }
      return;
    }
    if (activeNav === "downloads") {
      // Open the selected download entry
      const entry = downloadEntries[downloadsIndex];
      if (entry) {
        invoke("launch_program", { path: entry.path }).catch(console.error);
      }
      return;
    }
    if (activeNav === "store") {
      if (selectedIndex >= storeGames.length) {
        // "Load More" slot
        loadStoreNextPage();
      } else {
        const game = storeGames[selectedIndex];
        if (game) addStoreGameToLibrary(game);
      }
      return;
    }
    if (focusZone === "sidebar") {
      const nav = NAV_ITEMS[sidebarIndex];
      setActiveNav(nav.id);
      if (nav.id === "library") setActiveTab("all");
      setSelectedIndex(0);
      setFocusZone("grid");
    } else if (focusZone === "tabs") {
      setActiveTab(TABS[tabIndex].id);
      setSelectedIndex(0);
      setFocusZone("grid");
    } else if (activeNav === "library") {
      handleConfirm();
    }
  }, [activeNav, settingsIndex, appActionIndex, theme, igdbClientId, igdbClientSecret, downloadsIndex, downloadEntries, storeGames, selectedIndex, addStoreGameToLibrary, focusZone, sidebarIndex, tabIndex, handleConfirm, runningActionIndex, runningGames, handleSwitchToGame, handleCloseGame, adjustTrayMouseSpeed, showControllerWarning, dismissControllerWarning]);

  // LB/RB cycle sidebar nav items
  const cycleSidebarLeft = useCallback(() => {
    if (showControllerWarning) return;
    const idx = NAV_ITEMS.findIndex((n) => n.id === activeNav);
    const newIdx = (idx - 1 + NAV_ITEMS.length) % NAV_ITEMS.length;
    const nav = NAV_ITEMS[newIdx];
    setActiveNav(nav.id);
    setSidebarIndex(newIdx);
    if (nav.id === "library") setActiveTab("all");
    setSelectedIndex(0);
  }, [activeNav, showControllerWarning]);

  const cycleSidebarRight = useCallback(() => {
    if (showControllerWarning) return;
    const idx = NAV_ITEMS.findIndex((n) => n.id === activeNav);
    const newIdx = (idx + 1) % NAV_ITEMS.length;
    const nav = NAV_ITEMS[newIdx];
    setActiveNav(nav.id);
    setSidebarIndex(newIdx);
    if (nav.id === "library") setActiveTab("all");
    setSelectedIndex(0);
  }, [activeNav, showControllerWarning]);

  // LT/RT cycle tabs (All Games, Installed, Favorites, Recent)
  const cycleTabLeft = useCallback(() => {
    if (showControllerWarning) return;
    const idx = TABS.findIndex((t) => t.id === activeTab);
    const newIdx = (idx - 1 + TABS.length) % TABS.length;
    setActiveTab(TABS[newIdx].id);
    setTabIndex(newIdx);
    setSelectedIndex(0);
  }, [activeTab, showControllerWarning]);

  const cycleTabRight = useCallback(() => {
    if (showControllerWarning) return;
    const idx = TABS.findIndex((t) => t.id === activeTab);
    const newIdx = (idx + 1) % TABS.length;
    setActiveTab(TABS[newIdx].id);
    setTabIndex(newIdx);
    setSelectedIndex(0);
  }, [activeTab, showControllerWarning]);

  /* ── Gamepad ── */
  useGamepad({
    onUp: navigateUp,
    onDown: navigateDown,
    onLeft: navigateLeft,
    onRight: navigateRight,
    onConfirm: showControllerWarning ? dismissControllerWarning : editingProgram ? () => {
      // 0 = name input (no-op, just focused), 1 = change cover (if igdb), 2/3 = cancel/save
      if (igdbConfigured) {
        if (editFocusIndex === 1) openImagePicker(editingProgram);
        else if (editFocusIndex === 2) closeEditModal();
        else if (editFocusIndex === 3) saveEdit();
        else saveEdit(); // default: save on A from name input
      } else {
        if (editFocusIndex === 1) closeEditModal();
        else if (editFocusIndex === 2) saveEdit();
        else saveEdit();
      }
    } : zoneConfirm,
    onBack: showControllerWarning ? dismissControllerWarning : editingProgram ? closeEditModal : () => setFocusZone("sidebar"),
    onAdd: showControllerWarning ? () => {} : handleAdd,
    onDelete: showControllerWarning ? () => {} : editingProgram ? closeEditModal : handleDelete,
    onLB: showControllerWarning ? () => {} : cycleSidebarLeft,
    onRB: showControllerWarning ? () => {} : cycleSidebarRight,
    onLT: showControllerWarning ? () => {} : cycleTabLeft,
    onRT: showControllerWarning ? () => {} : cycleTabRight,
    onSelect: showControllerWarning ? () => {} : toggleFavorite,
  }, controllerMode && appInteractive);

  /* ── Keyboard ── */
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (showSplash) return;
      if (showControllerWarning) {
        if (e.key === "Escape" || e.key === "Enter") {
          e.preventDefault();
          dismissControllerWarning();
        }
        return;
      }
      if (showOnboarding) {
        switch (e.key) {
          case "Escape":
            e.preventDefault();
            localStorage.setItem(ONBOARDING_STORAGE_KEY, "true");
            setShowOnboarding(false);
            setOnboardingStep(0);
            break;
          case "ArrowLeft":
            e.preventDefault();
            setOnboardingStep((step) => Math.max(0, step - 1));
            break;
          case "ArrowRight":
          case "Enter":
            e.preventDefault();
            if (onboardingStep >= ONBOARDING_STEPS.length - 1) {
              localStorage.setItem(ONBOARDING_STORAGE_KEY, "true");
              setShowOnboarding(false);
              setOnboardingStep(0);
            } else {
              setOnboardingStep((step) => Math.min(ONBOARDING_STEPS.length - 1, step + 1));
            }
            break;
        }
        return;
      }
      if (e.key === "Escape") { if (editingProgram) closeEditModal(); return; }
      if (document.activeElement?.tagName === "INPUT") return;

      switch (e.key) {
        case "ArrowUp": e.preventDefault(); navigateUp(); break;
        case "ArrowDown": e.preventDefault(); navigateDown(); break;
        case "ArrowLeft": e.preventDefault(); navigateLeft(); break;
        case "ArrowRight": e.preventDefault(); navigateRight(); break;
        case "Enter": zoneConfirm(); break;
        case "Delete": handleDelete(); break;
        case "f": toggleFavorite(); break;
        case "e": if (selectedIndex < filteredPrograms.length) openEditModal(filteredPrograms[selectedIndex]); break;
        case "/": e.preventDefault(); searchInputRef.current?.focus(); break;
        case "Tab": {
          e.preventDefault();
          const zones: FocusZone[] = runningGames.length > 0 ? ["sidebar", "running", "tabs", "grid"] : ["sidebar", "tabs", "grid"];
          const idx = zones.indexOf(focusZone);
          setFocusZone(zones[e.shiftKey ? (idx - 1 + zones.length) % zones.length : (idx + 1) % zones.length]);
          break;
        }
      }
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [navigateUp, navigateDown, navigateLeft, navigateRight, zoneConfirm, handleDelete, toggleFavorite, editingProgram, closeEditModal, openEditModal, filteredPrograms, selectedIndex, focusZone, runningGames.length, showSplash, showOnboarding, onboardingStep, showControllerWarning, dismissControllerWarning]);

  // Sync tab index when activeTab changes
  useEffect(() => { setTabIndex(TABS.findIndex((t) => t.id === activeTab)); }, [activeTab]);
  useEffect(() => { setSidebarIndex(NAV_ITEMS.findIndex((n) => n.id === activeNav)); }, [activeNav]);

  return (
    <div className={`app-shell ${controllerMode ? "controller-mode" : "kb-mode"}`}>
      {showSplash && (
        <div className="splash-screen" role="status" aria-live="polite" aria-label="Loading GameKiosk">
          <div className="splash-card">
            <div className="splash-logo" aria-hidden="true">
              <img src="/icon.ico" alt="" />
            </div>
            <div className="splash-name">GameKiosk</div>
            <div className="splash-subtitle">Loading your library</div>
            <div className="splash-loading" aria-hidden="true">
              <span />
              <span />
              <span />
            </div>
          </div>
        </div>
      )}
      {showOnboarding && !showSplash && (
        <div className="onboarding-overlay" role="dialog" aria-modal="true" aria-labelledby="onboarding-title">
          <div className="onboarding-card" ref={onboardingRef} tabIndex={-1}>
            <div className="onboarding-progress" aria-label="Onboarding steps">
              {ONBOARDING_STEPS.map((step, index) => (
                <button
                  key={step.title}
                  className={`onboarding-dot ${index === onboardingStep ? "active" : ""}`}
                  onClick={() => setOnboardingStep(index)}
                  aria-label={`Go to step ${index + 1}: ${step.title}`}
                  aria-pressed={index === onboardingStep}
                  type="button"
                />
              ))}
            </div>
            <div className="onboarding-content">
              <div className="onboarding-kicker">First launch guide</div>
              <h2 id="onboarding-title">{ONBOARDING_STEPS[onboardingStep].title}</h2>
              <p>{ONBOARDING_STEPS[onboardingStep].body}</p>
              <ul>
                {ONBOARDING_STEPS[onboardingStep].points.map((point) => (
                  <li key={point}>{point}</li>
                ))}
              </ul>
            </div>
            <div className="onboarding-footer">
              <button
                className="modal-btn modal-btn-secondary"
                type="button"
                onClick={() => {
                  localStorage.setItem(ONBOARDING_STORAGE_KEY, "true");
                  setShowOnboarding(false);
                  setOnboardingStep(0);
                }}
              >
                Skip
              </button>
              <div className="onboarding-footer-spacer" />
              <button
                className="modal-btn modal-btn-secondary"
                type="button"
                onClick={() => setOnboardingStep((step) => Math.max(0, step - 1))}
                disabled={onboardingStep === 0}
              >
                Back
              </button>
              <button
                className="modal-btn modal-btn-primary"
                type="button"
                onClick={() => {
                  if (onboardingStep >= ONBOARDING_STEPS.length - 1) {
                    localStorage.setItem(ONBOARDING_STORAGE_KEY, "true");
                    setShowOnboarding(false);
                    setOnboardingStep(0);
                  } else {
                    setOnboardingStep((step) => Math.min(ONBOARDING_STEPS.length - 1, step + 1));
                  }
                }}
              >
                {onboardingStep >= ONBOARDING_STEPS.length - 1 ? "Get started" : "Next"}
              </button>
            </div>
          </div>
        </div>
      )}
      {showControllerWarning && !showSplash && !showOnboarding && (
        <div className="modal-overlay controller-warning-overlay" onClick={dismissControllerWarning} role="dialog" aria-modal="true" aria-labelledby="controller-warning-title">
          <div className="modal controller-warning-modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2 id="controller-warning-title">Controller Keys Reserved</h2>
              <button className="modal-close" onClick={dismissControllerWarning} aria-label="Close warning">âœ•</button>
            </div>
            <div className="modal-body controller-warning-body">
              <div className="warning-badge" aria-hidden="true">!</div>
              <div className="warning-copy">
                <p>START and SELECT are taken by GameKiosk while the launcher is active.</p>
                <p>Change those bindings in the game if you need those buttons for gameplay.</p>
              </div>
              <ul className="warning-list">
                <li>START opens launcher actions such as editing and tray cursor toggles.</li>
                <li>SELECT is reserved for launcher navigation and favorites.</li>
              </ul>
            </div>
            <div className="modal-footer">
              <button className="modal-btn modal-btn-primary" onClick={dismissControllerWarning}>Got it</button>
            </div>
          </div>
        </div>
      )}
      <div className="sr-only" role="status" aria-live="polite" aria-atomic="true">{message}</div>
      {message && <div className="toast" role="alert">{message}</div>}

      {/* Sidebar */}
      <nav className={`sidebar ${focusZone === "sidebar" ? "zone-active" : ""}`} aria-label="Main navigation">
        <div className="sidebar-logo" aria-hidden="true">K</div>
        {NAV_ITEMS.map((item, i) => (
          <button
            key={item.id}
            className={`sidebar-btn ${activeNav === item.id ? "active" : ""} ${focusZone === "sidebar" && sidebarIndex === i ? "zone-focused" : ""}`}
            onClick={() => {
              setActiveNav(item.id);
              if (item.id === "library") setActiveTab("all");
              setSelectedIndex(0);
            }}
            aria-label={item.label}
            aria-current={activeNav === item.id ? "page" : undefined}
            title={item.label}
          >
            <span className="sidebar-icon" aria-hidden="true">{item.icon}</span>
            <span className="sidebar-label">{item.label}</span>
          </button>
        ))}
        {controllerMode && <div className="sidebar-zone-hint" aria-hidden="true">LB/RB</div>}
      </nav>

      <main className="main-content" aria-label="Game library">
        {/* Top Bar */}
        <header className="top-bar">
          <h1 className="brand">GameKiosk</h1>
          <div className="search-bar" role="search">
            <label htmlFor="search-input" className="sr-only">Search games</label>
            <span className="search-icon" aria-hidden="true">⌕</span>
            <input
              id="search-input"
              ref={searchInputRef}
              type="search"
              placeholder={controllerMode ? "Search games..." : "Search games... (press /)"}
              value={activeNav === "store" ? storeSearch : searchQuery}
              onChange={(e) => { if (activeNav === "store") setStoreSearch(e.target.value); else setSearchQuery(e.target.value); setSelectedIndex(0); }}
              onKeyDown={(e) => { if (e.key === "Escape") { (e.target as HTMLInputElement).blur(); if (activeNav === "store") setStoreSearch(""); else setSearchQuery(""); } }}
              aria-label="Search games directory"
            />
          </div>
          {controllerMode ? (
            <div className="controller-hints" aria-hidden="true">
              <span className="hint-btn a">A</span> Launch
              <span className="hint-btn b">B</span> Back
              <span className="hint-btn x">X</span> Remove
              <span className="hint-btn y">Y</span> Add
              <span className="hint-bumper">LB/RB</span> Pages
              <span className="hint-bumper">LT/RT</span> Tabs
              <span className="hint-bumper">START</span> Edit
              <span className="hint-bumper">SELECT</span> Fav
            </div>
          ) : (
            <div className="keyboard-hints" aria-hidden="true">
              <kbd>/</kbd> Search <kbd>Tab</kbd> Zones <kbd>F</kbd> Fav <kbd>E</kbd> Edit <kbd>Del</kbd> Remove <kbd>↵</kbd> Launch
            </div>
          )}
        </header>

        {/* Running Games Bar */}
        {runningGames.length > 0 && (
          <div className={`running-bar ${focusZone === "running" ? "zone-active" : ""}`} role="region" aria-label="Running games">
            <span className="running-bar-label">RUNNING</span>
            <div className="running-bar-games">
              {runningGames.map((rg, gameIdx) => (
                <div key={rg.program_id} className={`running-chip ${rg.active ? "active" : "muted"}`}>
                  <span className="running-chip-dot" />
                  <span className="running-chip-name">{rg.program_name}</span>
                  <button
                    className={`running-chip-btn ${focusZone === "running" && runningActionIndex === (gameIdx * 2) ? "zone-focused" : ""}`}
                    onMouseEnter={() => { setFocusZone("running"); setRunningActionIndex(gameIdx * 2); }}
                    onClick={() => handleSwitchToGame(rg.program_id)}
                    title="Switch to game"
                    aria-label={`Switch to ${rg.program_name}`}
                  >{rg.active ? "▶" : "⏯"}</button>
                  <button
                    className={`running-chip-btn running-chip-close ${focusZone === "running" && runningActionIndex === (gameIdx * 2 + 1) ? "zone-focused" : ""}`}
                    onMouseEnter={() => { setFocusZone("running"); setRunningActionIndex(gameIdx * 2 + 1); }}
                    onClick={() => handleCloseGame(rg.program_id)}
                    title="Close game"
                    aria-label={`Close ${rg.program_name}`}
                  >✕</button>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Tabs */}
        {(activeNav === "library") && (
          <div className={`tabs-row ${focusZone === "tabs" ? "zone-active" : ""}`} role="tablist" aria-label="Filter games">
            <div className="tabs">
              {TABS.map((tab, i) => (
                <button
                  key={tab.id}
                  role="tab"
                  className={`tab ${activeTab === tab.id ? "active" : ""} ${focusZone === "tabs" && tabIndex === i ? "zone-focused" : ""}`}
                  onClick={() => { setActiveTab(tab.id); setSelectedIndex(0); }}
                  aria-selected={activeTab === tab.id}
                  aria-controls="game-grid"
                  tabIndex={activeTab === tab.id ? 0 : -1}
                  onKeyDown={(e) => {
                    const ids = TABS.map((t) => t.id);
                    const idx = ids.indexOf(activeTab);
                    if (e.key === "ArrowRight") { e.preventDefault(); setActiveTab(ids[(idx + 1) % ids.length]); setSelectedIndex(0); }
                    if (e.key === "ArrowLeft") { e.preventDefault(); setActiveTab(ids[(idx - 1 + ids.length) % ids.length]); setSelectedIndex(0); }
                  }}
                >
                  {tab.label}
                </button>
              ))}
            </div>
            <button className="platform-badge" aria-label="Platform filter" tabIndex={0}>Platform</button>
          </div>
        )}

        {activeNav === "store" && (
          <div className="tabs-row">
            <div className="tabs">
              <span className="tab active" role="heading" aria-level={2}>Browse Games</span>
              {storeGames.length > 0 && <span className="tab" style={{ cursor: "default", color: "var(--text-tertiary)" }}>{storeGames.length} games</span>}
            </div>
            {storeLoading && <div className="loading-indicator" role="status" aria-live="polite">Loading...</div>}
          </div>
        )}

        {/* Library Grid */}
        {(activeNav === "library") && (
          <div
            id="game-grid"
            ref={gridRef}
            className={`game-grid ${focusZone === "grid" ? "zone-active" : ""}`}
            role="grid"
            aria-label={`${activeTab} games, ${filteredPrograms.length} items`}
          >
            {filteredPrograms.map((program, index) => (
              <button
                key={program.id}
                ref={(el) => { if (el) cardRefs.current.set(index, el); }}
                className={`game-card ${index === selectedIndex && focusZone === "grid" ? "selected" : ""}`}
                onClick={() => { setSelectedIndex(index); setFocusZone("grid"); handleConfirm(); }}
                onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); openEditModal(program); }}
                onMouseEnter={() => { setSelectedIndex(index); setFocusZone("grid"); }}
                onFocus={() => { setSelectedIndex(index); setFocusZone("grid"); }}
                role="gridcell"
                aria-label={`${program.name}${program.favorite ? ", favorited" : ""}. ${program.hoursPlayed ? program.hoursPlayed + " hours played" : "Ready to launch"}.`}
                aria-selected={index === selectedIndex && focusZone === "grid"}
                tabIndex={index === selectedIndex ? 0 : -1}
              >
                <div className="card-cover" style={{ background: COVER_GRADIENTS[index % COVER_GRADIENTS.length] }}>
                  {resolvedCovers[program.id] ? (
                    <img
                      className="card-cover-img"
                      src={resolvedCovers[program.id]}
                      alt=""
                      loading="lazy"
                      onError={(e) => { (e.target as HTMLImageElement).style.display = "none"; }}
                    />
                  ) : (program.coverUrl || program.cover_url) ? (
                    <div className="card-cover-loading" aria-hidden="true" />
                  ) : (
                    <div className="card-cover-icon" aria-hidden="true">🎮</div>
                  )}
                  {program.installed === false && <div className="card-badge not-installed" aria-hidden="true">NOT INSTALLED</div>}
                  {program.favorite && <div className="card-fav" aria-hidden="true">★</div>}
                  {runningGames.some((rg) => rg.program_id === program.id) && (
                    <div className={`card-running ${runningGames.find((rg) => rg.program_id === program.id)?.active ? "active" : "suspended"}`} aria-hidden="true">
                      {runningGames.find((rg) => rg.program_id === program.id)?.active ? "▶ PLAYING" : "⏸ SUSPENDED"}
                    </div>
                  )}
                </div>
                <div className="card-info">
                  <div className="card-title">{program.name}</div>
                  <div className="card-meta">{program.hoursPlayed ? `${program.hoursPlayed} hrs played` : "Ready to launch"}</div>
                </div>
              </button>
            ))}
            <button
              ref={(el) => { if (el) cardRefs.current.set(filteredPrograms.length, el); }}
              className={`game-card add-card ${selectedIndex === filteredPrograms.length && focusZone === "grid" ? "selected" : ""}`}
              onClick={handleAdd}
              onMouseEnter={() => { setSelectedIndex(filteredPrograms.length); setFocusZone("grid"); }}
              onFocus={() => { setSelectedIndex(filteredPrograms.length); setFocusZone("grid"); }}
              role="gridcell"
              aria-label="Add a new game."
              aria-selected={selectedIndex === filteredPrograms.length && focusZone === "grid"}
              tabIndex={selectedIndex === filteredPrograms.length ? 0 : -1}
            >
              <div className="card-cover add-cover"><div className="add-icon" aria-hidden="true">+</div></div>
              <div className="card-info">
                <div className="card-title">Add Game</div>
                <div className="card-meta">Browse files</div>
              </div>
            </button>
          </div>
        )}

        {/* Downloads */}
        {activeNav === "downloads" && (
          <div className="settings-view" role="region" aria-label="Downloads">
            <div className="settings-card">
              <h2>DOWNLOADS</h2>
              {downloadsLoading && <div className="loading-indicator" role="status">Loading...</div>}
              {!downloadsLoading && downloadEntries.length === 0 && (
                <div className="empty-state" style={{ padding: 40 }}><p>Downloads folder is empty.</p></div>
              )}
              {!downloadsLoading && downloadEntries.length > 0 && (
                <div className="downloads-list" role="list">
                  {downloadEntries.map((entry, i) => (
                    <button
                      key={entry.path}
                      className={`downloads-item ${i === downloadsIndex && controllerMode ? "zone-focused" : ""}`}
                      role="listitem"
                      onClick={() => invoke("launch_program", { path: entry.path }).catch(console.error)}
                      onMouseEnter={() => setDownloadsIndex(i)}
                      onFocus={() => setDownloadsIndex(i)}
                      tabIndex={i === downloadsIndex ? 0 : -1}
                    >
                      <span className="downloads-icon" aria-hidden="true">{entry.is_dir ? "📁" : "📄"}</span>
                      <span className="downloads-name">{entry.name}</span>
                      <span className="downloads-size">{entry.is_dir ? "" : formatFileSize(entry.size)}</span>
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>
        )}

        {/* Store Grid */}
        {activeNav === "store" && (
          <div className="store-scroll-area">
            <div className="game-grid zone-active" role="grid" aria-label={`Store games, ${storeGames.length} items`}>
              {storeGames.map((game, index) => (
                <div key={`${game.id}-${index}`} className={`game-card store-card ${index === selectedIndex ? "selected" : ""}`} role="gridcell" tabIndex={0}
                  aria-label={`${game.name}. Rating ${game.rating}. Released ${game.released || "unknown"}.`}
                  onMouseEnter={() => setSelectedIndex(index)} onFocus={() => setSelectedIndex(index)}
                >
                  <div className="card-cover" style={{ background: COVER_GRADIENTS[index % COVER_GRADIENTS.length] }}>
                    {game.background_image ? (
                      <img className="card-cover-img" src={game.background_image} alt="" loading="lazy" onError={(e) => { (e.target as HTMLImageElement).style.display = "none"; }} />
                    ) : (
                      <div className="card-cover-icon" aria-hidden="true">🎮</div>
                    )}
                    {(game.rating ?? 0) >= 4 && <div className="card-fav" aria-hidden="true">★</div>}
                  </div>
                  <div className="card-info">
                    <div className="card-title">{game.name}</div>
                    <div className="card-meta-row">
                      <span className="card-meta">{(game.rating ?? 0) > 0 ? `★ ${game.rating}` : ""} {game.released ? `· ${game.released.slice(0, 4)}` : ""}</span>
                      <button
                        className="store-add-btn"
                        onClick={(e) => { e.stopPropagation(); addStoreGameToLibrary(game); }}
                        aria-label={`Get ${game.name}`}
                        title="Get game"
                      >Get</button>
                    </div>
                  </div>
                </div>
              ))}
              {storeGames.length === 0 && !storeLoading && (
                <div className="empty-state" role="status">
                  {igdbConfigured
                    ? <p>Search for games or browse popular titles.</p>
                    : <p>Set up your IGDB API key in <button style={{ background: "none", border: "none", color: "var(--accent)", cursor: "pointer", fontFamily: "inherit", fontSize: "inherit", fontWeight: 600, padding: 0 }} onClick={() => setActiveNav("settings")}>Settings</button> to browse games.</p>
                  }
                </div>
              )}
            </div>
            {(storeHasMore || storeLoading) && !storeSearch.trim() && storeGames.length > 0 && (
              <div className="store-pagination">
                <button
                  className={`store-page-btn ${selectedIndex >= storeGames.length ? "selected" : ""}`}
                  onClick={loadStoreNextPage}
                  disabled={storeLoading}
                >
                  {storeLoading ? "Loading..." : "Load More Games"}
                </button>
              </div>
            )}
          </div>
        )}

        {/* Settings */}
        {activeNav === "settings" && (
          <div className="settings-view" role="region" aria-label="Settings">
            <div className="settings-card">
              <h2>SETTINGS</h2>
              <div className={`setting-row ${controllerMode && settingsIndex === 0 ? "zone-focused" : ""}`}>
                <label>Theme</label>
                <div className="theme-picker">
                  {THEMES.map((t) => (
                    <button
                      key={t.id}
                      className={`theme-option ${theme === t.id ? "active" : ""}`}
                      onClick={() => setTheme(t.id)}
                      aria-label={`${t.label} theme`}
                      aria-pressed={theme === t.id}
                    >
                      <div className="theme-swatch" style={{ background: t.swatch }} />
                      <span className="theme-option-label">{t.label}</span>
                    </button>
                  ))}
                </div>
              </div>
              <div className={`setting-row ${controllerMode && settingsIndex === 1 ? "zone-focused" : ""}`}>
                <label htmlFor="controller-toggle">Controller Mode</label>
                <button
                  id="controller-toggle"
                  className={`toggle-btn ${controllerMode ? "on" : ""}`}
                  onClick={() => setControllerMode((v) => !v)}
                  role="switch"
                  aria-checked={controllerMode}
                  aria-label={`Controller mode is ${controllerMode ? "on" : "off"}`}
                >
                  <span className="toggle-knob" />
                  <span className="toggle-label">{controllerMode ? "ON" : "OFF"}</span>
                </button>
                <small>Optimizes UI for gamepad navigation with LB/RB zone switching</small>
              </div>
              <div className={`setting-row ${controllerMode && settingsIndex === 2 ? "zone-focused" : ""}`}>
                <label>Tray Cursor Speed</label>
                <div className="setting-inline-controls">
                  <button
                    className="modal-btn modal-btn-secondary speed-btn"
                    onClick={() => adjustTrayMouseSpeed(-TRAY_MOUSE_SPEED_STEP)}
                    aria-label="Decrease tray cursor speed"
                  >-</button>
                  <span className="speed-readout">{Math.round(trayMouseSpeed * 100)}%</span>
                  <button
                    className="modal-btn modal-btn-secondary speed-btn"
                    onClick={() => adjustTrayMouseSpeed(TRAY_MOUSE_SPEED_STEP)}
                    aria-label="Increase tray cursor speed"
                  >+</button>
                </div>
                <small>Use Left/Right to adjust speed. Press START anytime to toggle tray cursor ({trayMouseEnabled ? "ON" : "OFF"}).</small>
              </div>
              <div className={`setting-row ${controllerMode && settingsIndex === 3 ? "zone-focused" : ""}`}>
                <label>IGDB API</label>
                {igdbConfigured ? (
                  <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                    <span style={{ fontSize: 12, color: "var(--text-secondary)" }}>✓ Connected</span>
                    <button
                      className="modal-btn modal-btn-secondary"
                      style={{ padding: "6px 12px", fontSize: 11 }}
                      onClick={() => { setSetupClientId(igdbClientId); setSetupClientSecret(igdbClientSecret); setSetupError(""); setShowApiSetup(true); }}
                    >Change</button>
                  </div>
                ) : (
                  <button
                    className="modal-btn modal-btn-primary"
                    style={{ padding: "8px 16px", fontSize: 12, alignSelf: "flex-start" }}
                    onClick={() => { setSetupClientId(""); setSetupClientSecret(""); setSetupError(""); setShowApiSetup(true); }}
                  >Set up IGDB API</button>
                )}
                <small>Required for cover art and store browsing. Free at dev.twitch.tv</small>
              </div>
              <div className={`setting-row ${controllerMode && settingsIndex === 4 ? "zone-focused" : ""}`}>
                <label>Library Stats</label>
                <div className="settings-stats">
                  <div className="settings-stat">
                    <span className="settings-stat-value">{programs.length}</span>
                    <span className="settings-stat-label">Total</span>
                  </div>
                  <div className="settings-stat">
                    <span className="settings-stat-value">{programs.filter((p) => p.installed !== false).length}</span>
                    <span className="settings-stat-label">Installed</span>
                  </div>
                  <div className="settings-stat">
                    <span className="settings-stat-value">{programs.filter((p) => p.favorite).length}</span>
                    <span className="settings-stat-label">Favorites</span>
                  </div>
                </div>
              </div>
              <div className={`setting-row ${controllerMode && settingsIndex === 5 ? "zone-focused" : ""}`}>
                <label>Application</label>
                <div style={{ display: "flex", gap: 8 }}>
                  <button
                    className={`modal-btn modal-btn-secondary ${controllerMode && settingsIndex === 5 && appActionIndex === 0 ? "zone-focused" : ""}`}
                    style={{ padding: "8px 16px", fontSize: 12 }}
                    onClick={() => invoke("hide_launcher")}
                    onMouseEnter={() => setAppActionIndex(0)}
                  >Minimize to Tray</button>
                  <button
                    className={`modal-btn modal-btn-secondary ${controllerMode && settingsIndex === 5 && appActionIndex === 1 ? "zone-focused" : ""}`}
                    style={{ padding: "8px 16px", fontSize: 12, color: "#f87171" }}
                    onClick={() => invoke("quit_app")}
                    onMouseEnter={() => setAppActionIndex(1)}
                  >Quit</button>
                </div>
                <small>Use Left/Right to choose Minimize or Quit, then press A. Minimize hides to system tray. Click tray icon to restore.</small>
              </div>
            </div>
          </div>
        )}

      </main>

      {/* Edit Modal */}
      {editingProgram && (
        <div className="modal-overlay" onClick={closeEditModal} role="dialog" aria-modal="true" aria-label={`Edit ${editingProgram.name}`}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2>Edit Game</h2>
              <button className="modal-close" onClick={closeEditModal} aria-label="Close edit dialog">✕</button>
            </div>
            <div className="modal-body">
              <div className="modal-field">
                <label htmlFor="edit-name">Game Name</label>
                <input id="edit-name" type="text" value={editName} onChange={(e) => setEditName(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") saveEdit(); }} className={`modal-input ${controllerMode && editFocusIndex === 0 ? "zone-focused" : ""}`} autoFocus aria-required="true" />
              </div>
              {igdbConfigured && (
                <button
                  className={`modal-btn modal-btn-secondary ${controllerMode && editFocusIndex === 1 ? "zone-focused" : ""}`}
                  style={{ alignSelf: "flex-start", display: "flex", alignItems: "center", gap: 6 }}
                  onClick={() => { if (editingProgram) openImagePicker(editingProgram); }}
                >
                  <span>📷</span> Change Cover
                </button>
              )}
            </div>
            <div className="modal-footer">
              {controllerMode && <div className="modal-controller-hints" aria-hidden="true"><span className="hint-btn a">A</span> Select <span className="hint-btn b">B</span> Cancel <span className="hint-bumper">START</span> Close</div>}
              <button className={`modal-btn modal-btn-secondary ${controllerMode && editFocusIndex === (igdbConfigured ? 2 : 1) ? "zone-focused" : ""}`} onClick={closeEditModal}>Cancel</button>
              <button className={`modal-btn modal-btn-primary ${controllerMode && editFocusIndex === (igdbConfigured ? 3 : 2) ? "zone-focused" : ""}`} onClick={saveEdit} disabled={!editName.trim()}>Save</button>
            </div>
          </div>
        </div>
      )}

      {/* IGDB API Setup Modal */}
      {showApiSetup && (
        <div className="modal-overlay" onClick={() => setShowApiSetup(false)} role="dialog" aria-modal="true" aria-label="IGDB API Setup">
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2>IGDB API Setup</h2>
              <button className="modal-close" onClick={() => setShowApiSetup(false)} aria-label="Close">✕</button>
            </div>
            <div className="modal-body">
              <div className="api-setup-instructions">
                <p style={{ fontSize: 13, color: "var(--text-secondary)", lineHeight: 1.6, marginBottom: 12 }}>
                  GameKiosk uses the IGDB database for game cover art and browsing. You need free Twitch developer credentials to connect.
                </p>
                <ol style={{ fontSize: 12, color: "var(--text-secondary)", lineHeight: 1.8, paddingLeft: 18, marginBottom: 16 }}>
                  <li>Go to <span style={{ color: "var(--accent)", fontWeight: 600, userSelect: "all" }}>dev.twitch.tv/console</span> and log in with Twitch</li>
                  <li>Click "Register Your Application"</li>
                  <li>Name it anything (e.g. "GameKiosk")</li>
                  <li>Set OAuth Redirect URL to <span style={{ color: "var(--accent)", fontWeight: 600, userSelect: "all" }}>http://localhost</span></li>
                  <li>Set Category to "Application Integration"</li>
                  <li>Click "Create", then "Manage" your new app</li>
                  <li>Copy the Client ID and generate a Client Secret</li>
                </ol>
              </div>
              <div className="modal-field">
                <label htmlFor="igdb-client-id">Client ID</label>
                <input
                  id="igdb-client-id"
                  type="text"
                  value={setupClientId}
                  onChange={(e) => setSetupClientId(e.target.value)}
                  placeholder="Your Twitch Client ID"
                  className="modal-input"
                  autoFocus
                  autoComplete="off"
                />
              </div>
              <div className="modal-field">
                <label htmlFor="igdb-client-secret">Client Secret</label>
                <input
                  id="igdb-client-secret"
                  type="password"
                  value={setupClientSecret}
                  onChange={(e) => setSetupClientSecret(e.target.value)}
                  placeholder="Your Twitch Client Secret"
                  className="modal-input"
                  autoComplete="off"
                  onKeyDown={(e) => { if (e.key === "Enter") handleSaveApiCredentials(); }}
                />
              </div>
              {setupError && <p style={{ fontSize: 12, color: "#f87171", fontWeight: 500 }}>{setupError}</p>}
            </div>
            <div className="modal-footer">
              <button className="modal-btn modal-btn-secondary" onClick={() => setShowApiSetup(false)}>Cancel</button>
              <button
                className="modal-btn modal-btn-primary"
                onClick={handleSaveApiCredentials}
                disabled={!setupClientId.trim() || !setupClientSecret.trim() || setupValidating}
              >{setupValidating ? "Validating..." : "Connect"}</button>
            </div>
          </div>
        </div>
      )}

      {/* Image Picker Modal */}
      {showImagePicker && imagePickerProgram && (
        <div className="modal-overlay" onClick={closeImagePicker} role="dialog" aria-modal="true" aria-label={`Choose cover for ${imagePickerProgram.name}`}>
          <div className="modal image-picker-modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2>Choose Cover — {imagePickerProgram.name}</h2>
              <button className="modal-close" onClick={closeImagePicker} aria-label="Close">✕</button>
            </div>
            <div className="modal-body">
              <div className="modal-search-row">
                <input
                  type="text"
                  value={imagePickerQuery}
                  onChange={(e) => setImagePickerQuery(e.target.value)}
                  onKeyDown={(e) => { if (e.key === "Enter") handleImagePickerSearch(); }}
                  placeholder="Search for game images..."
                  className="modal-input"
                  autoFocus
                />
                <button
                  className="modal-btn modal-btn-secondary"
                  onClick={handleImagePickerSearch}
                  disabled={imagePickerLoading || !imagePickerQuery.trim()}
                >{imagePickerLoading ? "..." : "Search"}</button>
              </div>
              {imagePickerLoading && <div className="loading-indicator" style={{ textAlign: "center", padding: 20 }}>Searching images...</div>}
              <div className="image-picker-grid" role="listbox" aria-label="Available images">
                {imagePickerResults.map((img, i) => (
                  <button
                    key={`${img.game_id}-${img.kind}-${i}`}
                    className="image-picker-item"
                    onClick={() => selectPickerImage(img.url)}
                    role="option"
                    aria-label={`${img.kind} from ${img.game_name}`}
                    title={`${img.game_name} — ${img.kind}`}
                  >
                    <img src={img.url} alt={`${img.game_name} ${img.kind}`} loading="lazy" />
                    <div className="image-picker-item-info">
                      <span className="image-picker-item-name">{img.game_name}</span>
                      <span className="image-picker-item-kind">{img.kind}</span>
                    </div>
                  </button>
                ))}
              </div>
              {!imagePickerLoading && imagePickerResults.length === 0 && (
                <div className="empty-state" style={{ padding: 40 }}>No images found. Try a different search.</div>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
