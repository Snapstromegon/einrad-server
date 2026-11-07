// Per-browser view preferences of the club portal (collapsed panels, hidden
// columns, filters). Storage may be unavailable (private mode), so failures
// fall back to the default.

const PREFIX = "cup.";

export const loadPref = <T>(key: string, fallback: T): T => {
  try {
    const raw = localStorage.getItem(PREFIX + key);
    return raw === null ? fallback : (JSON.parse(raw) as T);
  } catch {
    return fallback;
  }
};

export const savePref = (key: string, value: unknown) => {
  try {
    localStorage.setItem(PREFIX + key, JSON.stringify(value));
  } catch {
    // Not stored; the view still works.
  }
};
