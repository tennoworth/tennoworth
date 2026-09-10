import { serializeSnapshot, deserializeSnapshot, type Snapshot, type SaveSnapshotInput } from '../domain/snapshot';


const KEY = 'wfminv:last-owned-v7';


export function saveSnapshot(input: SaveSnapshotInput): void {
  try {
    localStorage.removeItem('wfminv:last-owned-v6');
    localStorage.setItem(KEY, serializeSnapshot(input, Date.now()));
  } catch (e) {
    console.warn('Could not persist inventory snapshot:', e);
  }
}


export function loadSnapshot(): Snapshot | null {
  try {
    return deserializeSnapshot(localStorage.getItem(KEY));
  } catch (e) {
    console.warn('Could not load inventory snapshot:', e);
    return null;
  }
}


export function clearSnapshot(): void {
  try {
    localStorage.removeItem(KEY);
  } catch {
    /* ignore */
  }
}
