import { vi } from "vitest";

// Node gives tests no localStorage (only with a file to keep it in): one in memory for each file.
class MemoryStorage {
  private items = new Map<string, string>();
  get length() {
    return this.items.size;
  }
  key = (index: number) => [...this.items.keys()][index] ?? null;
  getItem = (key: string) => this.items.get(key) ?? null;
  setItem = (key: string, value: string) => void this.items.set(key, value);
  removeItem = (key: string) => void this.items.delete(key);
  clear = () => this.items.clear();
}

vi.stubGlobal("localStorage", new MemoryStorage());
