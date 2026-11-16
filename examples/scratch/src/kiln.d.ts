/** The Kiln IPC bridge, installed before any page script runs. */
declare global {
  interface Window {
    kiln: {
      /** Call a Rust command. Rejects if the handler errored or is unknown. */
      invoke<T = unknown>(method: string, payload?: unknown): Promise<T>;
      /** Subscribe to a Rust -> JS event. Returns an unsubscribe function. */
      listen<T = unknown>(event: string, fn: (payload: T) => void): () => void;
      /** Which platform this page is running on. Use it to hide affordances that
       *  cannot work there, rather than offering buttons that error. */
      platform: "linux" | "windows" | "macos" | "bsd" | "android" | "ios";
    };
  }
}

export {};
