import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AppConfig, Message, Profile } from "./types";

type Bytes = Uint8Array;

export const api = {
  bootstrap: () => invoke<{
    profile: Profile;
    config: AppConfig;
    peers: import("./types").PeerView[];
    conversations: import("./types").Conversation[];
  }>("get_bootstrap"),
  listMessages: (fp: string, limit = 300) =>
    invoke<Message[]>("list_messages", { fp, limit }),
  openSession: (fp: string) =>
    invoke<{ ok: boolean; verified: boolean }>("open_session", { fp }),
  confirmPeer: (fp: string) => invoke<void>("confirm_peer", { fp }),
  setBlocked: (fp: string, blocked: boolean) =>
    invoke<void>("set_blocked", { fp, blocked }),
  forgetPeer: (fp: string) => invoke<void>("forget_peer", { fp }),
  sendText: (fp: string, body: string) => invoke<Message>("send_text", { fp, body }),
  typing: (fp: string) => invoke<void>("typing", { fp }),
  markRead: (fp: string) => invoke<void>("mark_read", { fp }),
  sendFiles: (fp: string, paths: string[]) =>
    invoke<Array<Record<string, unknown>>>("send_files", { fp, paths }),
  sendMedia: (fp: string, bytes: Bytes, name: string, kind: string) =>
    invoke<Message>("send_media", { fp, bytes: Array.from(bytes), name, kind }),
  cancelTransfer: (fid: string) => invoke<void>("cancel_transfer", { fid }),
  getAvatar: (fp: string): Promise<string | null> =>
    invoke<string | null>("get_avatar", { fp }).catch(() => null),
  setProfile: (nickname: string, avatarData?: string | null) =>
    invoke<Profile>("set_profile", { nickname, avatarData: avatarData ?? null }),
  setSettings: (patch: Record<string, unknown>) =>
    invoke<AppConfig>("set_settings", { patch }),
  clearHistory: (fp?: string | null) => invoke<void>("clear_history", { fp: fp ?? null }),
  revealPath: (path: string) => invoke<void>("reveal_path", { path }),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  quitApp: () => invoke<void>("quit_app"),
};

export interface Events {
  peers: import("./types").PeerView[];
  "message-new": Message;
  "message-state": { mid: string; state: string };
  session: { fp: string; state: "open" | "closed"; verified?: boolean };
  typing: { fp: string };
  transfer: { mid: string; fid: string; dir: number; sent: number; total: number };
  "transfer-done": { fid: string; mid: string; ok: boolean };
  alert: { code: string; message?: string; nick?: string; knownFp?: string; newFp?: string };
  "call-signal": { fp: string; payload: string };
}

export function wireListeners(handlers: {
  [K in keyof Events]: (payload: Events[K]) => void;
}): () => void {
  const unlistens: (() => void)[] = [];
  let cancelled = false;
  for (const name of Object.keys(handlers) as (keyof Events)[]) {
    const handler = handlers[name] as (payload: unknown) => void;
    void listen(name, (e) => handler(e.payload)).then((un) => {
      if (cancelled) un();
      else unlistens.push(un);
    });
  }
  return () => {
    cancelled = true;
    unlistens.forEach((u) => u());
  };
}
