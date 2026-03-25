/**
 * IGDB API client — all HTTP calls go through the Rust backend to avoid CORS.
 */
import { invoke } from "@tauri-apps/api/core";

export interface IgdbGame {
  id: number;
  name: string;
  background_image: string | null;
  rating?: number;
  released: string | null;
}

function formatReleased(raw: string | null): string | null {
  if (!raw) return null;
  // Backend returns unix timestamp as string
  const ts = parseInt(raw, 10);
  if (isNaN(ts)) return raw;
  return new Date(ts * 1000).getFullYear().toString();
}

function mapResult(r: any): IgdbGame {
  return {
    id: r.id,
    name: r.name,
    background_image: r.background_image ?? null,
    rating: r.rating ?? undefined,
    released: formatReleased(r.released),
  };
}

export async function searchGames(query: string): Promise<IgdbGame[]> {
  if (!query.trim()) return [];
  try {
    const results = await invoke<any[]>("igdb_search", { query });
    return results.map(mapResult);
  } catch {
    return [];
  }
}

export async function fetchPopularGames(limit = 20, offset = 0): Promise<IgdbGame[]> {
  try {
    const results = await invoke<any[]>("igdb_popular", { limit, offset });
    return results.map(mapResult);
  } catch {
    return [];
  }
}

export async function fetchGameImage(gameName: string): Promise<string | null> {
  const results = await searchGames(gameName);
  return results[0]?.background_image ?? null;
}

export async function validateCredentials(
  clientId: string,
  clientSecret: string
): Promise<boolean> {
  try {
    return await invoke<boolean>("igdb_validate", {
      clientId,
      clientSecret,
    });
  } catch {
    return false;
  }
}

export interface IgdbImage {
  game_id: number;
  game_name: string;
  url: string;
  kind: "screenshot" | "cover" | "artwork";
}

export async function searchGameImages(query: string): Promise<IgdbImage[]> {
  if (!query.trim()) return [];
  try {
    return await invoke<IgdbImage[]>("igdb_game_images", { query });
  } catch {
    return [];
  }
}
