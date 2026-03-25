export interface Program {
  id: string;
  name: string;
  path: string;
  cover_url?: string | null;
  favorite: boolean;
  hours_played?: number | null;
  // Frontend-only transient fields (not persisted in Rust)
  coverUrl?: string;
  hoursPlayed?: number;
  installed?: boolean;
  lastPlayed?: number;
}

export type TabFilter = "all" | "installed" | "favorites" | "recent";
export type NavItem = "library" | "downloads" | "store" | "settings";
