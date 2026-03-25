const CACHE_PREFIX = "img_cache_";
const CACHE_INDEX_KEY = "img_cache_index";
const MAX_CACHE_ENTRIES = 100;

interface CacheEntry {
  url: string;
  dataUrl: string;
  timestamp: number;
}

/** Get all cached keys sorted by age (oldest first) */
function getCacheIndex(): string[] {
  try {
    return JSON.parse(localStorage.getItem(CACHE_INDEX_KEY) || "[]");
  } catch {
    return [];
  }
}

function saveCacheIndex(index: string[]) {
  localStorage.setItem(CACHE_INDEX_KEY, JSON.stringify(index));
}

/** Evict oldest entries if over limit */
function evictIfNeeded() {
  const index = getCacheIndex();
  while (index.length > MAX_CACHE_ENTRIES) {
    const oldest = index.shift()!;
    localStorage.removeItem(CACHE_PREFIX + oldest);
  }
  saveCacheIndex(index);
}

/** Get a cached image data URL by its original URL */
export function getCachedImage(url: string): string | null {
  try {
    const raw = localStorage.getItem(CACHE_PREFIX + url);
    if (!raw) return null;
    const entry: CacheEntry = JSON.parse(raw);
    return entry.dataUrl;
  } catch {
    return null;
  }
}

/** Convert a remote image URL to a base64 data URL and cache it */
export async function cacheImage(url: string): Promise<string> {
  // Return from cache if available
  const cached = getCachedImage(url);
  if (cached) return cached;

  try {
    return new Promise<string>((resolve) => {
      // First try with crossOrigin to enable canvas caching
      const img = new Image();
      img.crossOrigin = "anonymous";

      const tryCacheToCanvas = () => {
        try {
          const canvas = document.createElement("canvas");
          const maxW = 800;
          const scale = Math.min(1, maxW / img.naturalWidth);
          canvas.width = img.naturalWidth * scale;
          canvas.height = img.naturalHeight * scale;
          const ctx = canvas.getContext("2d")!;
          ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
          const dataUrl = canvas.toDataURL("image/webp", 0.85);

          try {
            const entry: CacheEntry = { url, dataUrl, timestamp: Date.now() };
            localStorage.setItem(CACHE_PREFIX + url, JSON.stringify(entry));
            const index = getCacheIndex().filter((k) => k !== url);
            index.push(url);
            saveCacheIndex(index);
            evictIfNeeded();
          } catch {
            const index = getCacheIndex();
            const half = Math.floor(index.length / 2);
            for (let i = 0; i < half; i++) {
              localStorage.removeItem(CACHE_PREFIX + index[i]);
            }
            saveCacheIndex(index.slice(half));
            try {
              localStorage.setItem(
                CACHE_PREFIX + url,
                JSON.stringify({ url, dataUrl, timestamp: Date.now() })
              );
            } catch { /* give up caching */ }
          }
          resolve(dataUrl);
        } catch {
          // Canvas tainted — just use the original URL (image still displays)
          resolve(url);
        }
      };

      img.onload = tryCacheToCanvas;
      img.onerror = () => {
        // CORS blocked with crossOrigin attribute — retry without it
        // Image will display but can't be cached to canvas
        const img2 = new Image();
        img2.onload = () => resolve(url); // just return the URL, it'll display fine
        img2.onerror = () => resolve(url);
        img2.src = url;
      };
      img.src = url;
    });
  } catch {
    return url;
  }
}

/** Preload + cache an image, returns the usable src (data URL or original) */
export function preloadImage(url: string): Promise<string> {
  const cached = getCachedImage(url);
  if (cached) return Promise.resolve(cached);
  return cacheImage(url);
}
