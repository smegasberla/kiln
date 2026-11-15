/** The Kiln IPC bridge, installed before any page script runs. */
declare global {
  interface Window {
    kiln: {
      /** Call a Rust command. Rejects if the handler errored or is unknown. */
      invoke<T = unknown>(method: string, payload?: unknown): Promise<T>;
      /** Subscribe to a Rust -> JS event. Returns an unsubscribe function. */
      listen<T = unknown>(event: string, fn: (payload: T) => void): () => void;
    };
  }
}


