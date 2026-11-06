import { useEffect, useState } from "react";
import { LIVE_LIMITS, emptyCounters, type LiveSnapshot, type LiveUpdate } from "../../shared/live";
import { initialTikTokState, type TikTokLiveState } from "../../shared/tiktok";

/**
 * Données du live (journal, chats, compteurs) et état TikTok, poussés par le
 * cœur de Batlay. Utilisés par la page Live, la Vue d'ensemble et la barre latérale.
 */

/** Snapshot initial puis mises à jour poussées par le cœur de Batlay. */
export function useLiveData(): LiveSnapshot {
  const [data, setData] = useState<LiveSnapshot>(() => ({ activity: [], chat: [], counters: emptyCounters(Date.now()) }));
  useEffect(() => {
    let alive = true;
    const pending: LiveUpdate[] = [];
    let loaded = false;
    const apply = (cur: LiveSnapshot, u: LiveUpdate): LiveSnapshot => {
      switch (u.type) {
        case "activity":
          return cur.activity.some((e) => e.id === u.entry.id) ? cur : { ...cur, activity: [...cur.activity, u.entry].slice(-LIVE_LIMITS.activity) };
        case "chat":
          return cur.chat.some((m) => m.id === u.message.id) ? cur : { ...cur, chat: [...cur.chat, u.message].slice(-LIVE_LIMITS.chat) };
        case "chat-remove": {
          const ids = new Set(u.ids);
          return { ...cur, chat: cur.chat.filter((m) => !ids.has(m.id)) };
        }
        case "counters":
          return { ...cur, counters: u.counters };
        case "reset":
          return { ...u.snapshot, chat: cur.chat };
      }
    };
    const off = window.batlay.live.onUpdate((u) => {
      if (!alive) return;
      if (!loaded) pending.push(u);
      else setData((cur) => apply(cur, u));
    });
    void window.batlay.live.getSnapshot().then((snap) => {
      if (!alive) return;
      loaded = true;
      setData(pending.reduce(apply, snap));
    });
    return () => {
      alive = false;
      off();
    };
  }, []);
  return data;
}

export function useTikTokState(): TikTokLiveState {
  const [state, setState] = useState<TikTokLiveState>(() => initialTikTokState());
  useEffect(() => {
    let alive = true;
    void window.batlay.tiktok.getState().then((s) => alive && setState(s)).catch(() => undefined);
    const off = window.batlay.tiktok.onState((s) => alive && setState(s));
    return () => {
      alive = false;
      off();
    };
  }, []);
  return state;
}

