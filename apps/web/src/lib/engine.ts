import type {
  ConnectErrorKind,
  ConnectStage,
  EngineEvent,
  RemoteDisplay,
  Route,
  SessionStats,
} from '@scrin/ui';
import { queryOptions, useMutation, useQueryClient } from '@tanstack/react-query';
import { useEffect, useReducer, useRef, useSyncExternalStore } from 'react';

import { host } from '../host';

export const myIdQuery = queryOptions({
  queryKey: ['engine', 'my-id'],
  queryFn: () => host.engine.getMyId(),
  staleTime: Number.POSITIVE_INFINITY,
});

export const codeQuery = queryOptions({
  queryKey: ['engine', 'code'],
  queryFn: () => host.engine.getCode(),
  // Refetch just after expiry so a fresh code appears without user action.
  refetchInterval: (q) => {
    const exp = q.state.data?.expiresAt;
    return exp === undefined ? false : Math.max(1000, exp - Date.now() + 250);
  },
});

export function useRegenerateCode() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => host.engine.regenerateCode(),
    onSuccess: (code) => {
      qc.setQueryData(codeQuery.queryKey, code);
    },
  });
}

/** Route param standing in for a dictated passphrase (the words never enter the URL). */
export const PHRASE_TARGET = 'phrase';

export const passphraseQuery = queryOptions({
  queryKey: ['engine', 'passphrase'],
  queryFn: async () => (await host.engine.getPassphrase?.()) ?? null,
  // Words change after each use and when the locator is renewed.
  refetchInterval: 5000,
});

/** `lang` shows the words in that language; `null` turns the passphrase off. */
export function useSetPassphrase() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (lang: string | null) => (await host.engine.setPassphrase?.(lang)) ?? null,
    onSuccess: (phrase) => {
      qc.setQueryData(passphraseQuery.queryKey, phrase);
    },
  });
}

interface ConnectState {
  sessionId: string | null;
  stage: ConnectStage;
  route: Route | null;
  sas: number[] | null;
  sasConfirmed: boolean;
  error: ConnectErrorKind | null;
}

const initialConnect: ConnectState = {
  sessionId: null,
  stage: 'locating',
  route: null,
  sas: null,
  sasConfirmed: false,
  error: null,
};

type ConnectAction =
  | { type: 'reset' }
  | { type: 'started'; sessionId: string }
  | { type: 'event'; event: EngineEvent }
  | { type: 'sas-confirmed' };

function connectReducer(state: ConnectState, action: ConnectAction): ConnectState {
  if (action.type === 'reset') return initialConnect;
  if (action.type === 'started') return { ...initialConnect, sessionId: action.sessionId };
  if (action.type === 'sas-confirmed') return { ...state, sasConfirmed: true };
  const e = action.event;
  if (e.sessionId !== state.sessionId) return state;
  if (e.type === 'stage') return { ...state, stage: e.stage, route: e.route ?? state.route };
  if (e.type === 'sas') return { ...state, sas: e.emoji };
  if (e.type === 'error') return { ...state, error: e.error };
  return state;
}

/**
 * Drives one outgoing connection attempt from the engine's event stream.
 * Retrying remounts the caller with a new `key`, so the hook stays one-shot.
 */
export function useConnectFlow(id: string, code: string | null) {
  const [state, dispatch] = useReducer(connectReducer, initialConnect);
  const cancelRef = useRef<() => void>(() => undefined);

  useEffect(() => {
    if (!code) return undefined;
    let alive = true;
    // Subscribe first so the first stage event is not missed.
    const off = host.engine.onEvent((event) => {
      if (alive) dispatch({ type: 'event', event });
    });
    dispatch({ type: 'reset' });
    // A passphrase carries its own secret: the words are the target.
    const [target, secret] = id === PHRASE_TARGET ? [code, ''] : [id, code];
    void host.engine.connect(target, secret).then((handle) => {
      if (!alive) {
        handle.cancel();
        return;
      }
      cancelRef.current = () => {
        handle.cancel();
      };
      dispatch({ type: 'started', sessionId: handle.sessionId });
    });
    return () => {
      alive = false;
      off();
      cancelRef.current();
    };
  }, [id, code]);

  const confirmSas = async (matches: boolean) => {
    if (!state.sessionId) return;
    if (matches) dispatch({ type: 'sas-confirmed' });
    await host.engine.confirmSas(state.sessionId, matches);
  };

  return {
    state,
    confirmSas,
    cancel: () => {
      cancelRef.current();
    },
  };
}

interface ChatMessage {
  from: 'remote' | 'local';
  text: string;
  at: number;
}

export interface SessionState {
  stats: SessionStats | null;
  displays: RemoteDisplay[];
  chat: ChatMessage[];
  ended: boolean;
}

const EMPTY_SESSION: SessionState = { stats: null, displays: [], chat: [], ended: false };

function sessionReducer(s: SessionState, e: EngineEvent): SessionState {
  if (e.type === 'stats') return { ...s, stats: e.stats };
  if (e.type === 'displays') return { ...s, displays: e.displays };
  if (e.type === 'chat')
    return { ...s, chat: [...s.chat, { from: e.from, text: e.text, at: e.at }] };
  if (e.type === 'ended') return { ...s, ended: true };
  return s;
}

/**
 * Session state lives outside React: the connect screen sees `connected` and
 * the display list before the session screen mounts, and those events must
 * not be lost in the hand-over.
 */
const sessions = new Map<string, SessionState>();
const subscribers = new Set<() => void>();
host.engine.onEvent((e) => {
  if (e.type === 'stage' || e.type === 'sas' || e.type === 'error') return;
  sessions.set(e.sessionId, sessionReducer(sessions.get(e.sessionId) ?? EMPTY_SESSION, e));
  for (const s of subscribers) s();
});

/** Live stats, displays and chat for an open session. */
export function useSessionEvents(sessionId: string | undefined): SessionState {
  return useSyncExternalStore(
    (onChange) => {
      subscribers.add(onChange);
      return () => {
        subscribers.delete(onChange);
      };
    },
    () => (sessionId ? (sessions.get(sessionId) ?? EMPTY_SESSION) : EMPTY_SESSION),
  );
}
