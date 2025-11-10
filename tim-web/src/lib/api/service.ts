import { browser } from '$app/environment';
import { createClient, type Client, Code, ConnectError } from '@connectrpc/connect';
import { createGrpcWebTransport } from '@connectrpc/connect-web';
import { create } from '@bufbuild/protobuf';
import {
	TimApi,
	AuthenticateReqSchema,
	ClientInfoSchema,
	TimiteSchema,
	type AuthenticateReq,
	type ClientInfo as RpcClientInfo,
	type SpaceUpdate as RpcSpaceUpdate,
	type Timite as RpcTimite
} from '../../gen/tim/api/g1/api_pb';
import type { ApiListener, ConnectionStateMessage, SpaceUpdateMessage } from '$lib/api/types';
import type { ConnectionState } from '$lib/models/session';

const CLIENT_ID_STORAGE_KEY = 'tim.client-id';
const SESSION_ID_STORAGE_KEY = 'tim.session-id';
const SESSION_HEADER = 'tim-session-id' as const;

const listeners = new Set<ApiListener>();
let client: Client<typeof TimApi> | null = null;
let streamController: AbortController | null = null;
let streamTask: Promise<void> | null = null;
let connectionState: ConnectionState = 'connecting';
let connectionEventCounter = 0;
let cachedClientId: string | null = null;
let cachedSessionId: string | null = null;
let sessionInit: Promise<string> | null = null;

const dispatch = (message: SpaceUpdateMessage) => {
	for (const listener of listeners) {
		listener(message);
	}
};

const createConnectionMessage = (state: ConnectionState): ConnectionStateMessage => ({
	type: 'connection.state',
	id: `connection:${connectionEventCounter++}`,
	payload: { state }
});

const emitConnectionState = (state: ConnectionState) => {
	if (connectionState === state) return;
	connectionState = state;
	dispatch(createConnectionMessage(state));
};

async function ensureClient(): Promise<Client<typeof TimApi>> {
	if (client) return client;

	const baseUrl = resolveBackendBaseUrl();
	if (!baseUrl) {
		throw new Error('Unable to resolve backend base URL for gRPC transport.');
	}

	client = createClient(
		TimApi,
		createGrpcWebTransport({
			baseUrl
		})
	);

	return client;
}

function ensureSubscription() {
	if (!browser) return;
	if (streamTask) return;
	if (listeners.size === 0) return;

	streamController = new AbortController();
	const { signal } = streamController;
	streamTask = runSubscription(signal).finally(() => {
		streamTask = null;
		streamController = null;
		if (connectionState !== 'connecting' && listeners.size > 0) {
			emitConnectionState('connecting');
		}
	});
}

function stopSubscription() {
	if (streamController) {
		streamController.abort();
		streamController = null;
	}
	streamTask = null;
	if (connectionState !== 'connecting') {
		emitConnectionState('connecting');
	}
}

async function runSubscription(signal: AbortSignal) {
	let retryCount = 0;

	while (!signal.aborted) {
		try {
			const rpcClient = await ensureClient();
			const sessionId = await ensureSessionId();
			const stream = rpcClient.subscribeToSpace(
				{ clientId: resolveClientId() },
				{
					signal,
					headers: buildSessionHeaders(sessionId)
				}
			);
			retryCount = 0;
			emitConnectionState('open');

			for await (const message of stream) {
				if (signal.aborted) break;
				const converted = translateSpaceUpdate(message);
				if (converted) {
					dispatch(converted);
				}
			}

			if (signal.aborted) {
				break;
			}

			emitConnectionState('reconnecting');
		} catch (error) {
			if (handleAuthFailure(error)) {
				continue;
			}
			if (signal.aborted) {
				break;
			}
			console.error('Failed to maintain backend subscription', error);
			emitConnectionState('reconnecting');
			retryCount = Math.min(retryCount + 1, 8);
		}

		const delay = Math.min(500 * Math.max(retryCount, 1), 5000);
		try {
			await wait(delay, signal);
		} catch {
			break;
		}
	}
}

function translateSpaceUpdate(message: RpcSpaceUpdate): SpaceUpdateMessage | null {
	const id = message.id || generateId();
	const event = message.event;

	if (!event || !event.case) {
		console.warn('Space update missing event payload', message);
		return null;
	}

	if (event.case === 'spaceNewMessage') {
		const payload = event.value?.message;
		if (!payload) return null;
		return {
			type: 'space.message',
			id,
			payload: {
				senderId: payload.senderId || 'system',
				content: payload.content ?? ''
			}
		};
	}

	console.warn('Received unsupported space update payload', message);
	return null;
}

function resolveClientId(): string {
	if (cachedClientId) return cachedClientId;

	const fallback = `client-${generateId()}`;
	if (!browser) {
		cachedClientId = fallback;
		return cachedClientId;
	}

	try {
		const stored = localStorage.getItem(CLIENT_ID_STORAGE_KEY);
		if (stored && stored.length > 0) {
			cachedClientId = stored;
			return cachedClientId;
		}

		const generated = generateId();
		localStorage.setItem(CLIENT_ID_STORAGE_KEY, generated);
		cachedClientId = generated;
		return cachedClientId;
	} catch {
		cachedClientId = fallback;
		return cachedClientId;
	}
}

async function ensureSessionId(): Promise<string> {
	if (cachedSessionId) return cachedSessionId;
	if (sessionInit) return sessionInit;

	sessionInit = (async () => {
		const stored = loadStoredSessionId();
		if (stored) {
			cachedSessionId = stored;
			return stored;
		}

		const rpcClient = await ensureClient();
		const response = await rpcClient.authenticate(buildAuthenticateRequest());
		const sessionId = response.session?.id;
		if (!sessionId) {
			throw new Error('Failed to acquire session id from backend.');
		}
		cachedSessionId = sessionId;
		persistSessionId(sessionId);
		return sessionId;
	})();

	try {
		return await sessionInit;
	} finally {
		sessionInit = null;
	}
}

function loadStoredSessionId(): string | null {
	if (!browser) return null;
	try {
		const stored = localStorage.getItem(SESSION_ID_STORAGE_KEY);
		return stored && stored.length > 0 ? stored : null;
	} catch {
		return null;
	}
}

function persistSessionId(id: string) {
	if (!browser) return;
	try {
		localStorage.setItem(SESSION_ID_STORAGE_KEY, id);
	} catch {
		/* ignore */
	}
}

function clearStoredSessionId() {
	cachedSessionId = null;
	if (!browser) return;
	try {
		localStorage.removeItem(SESSION_ID_STORAGE_KEY);
	} catch {
		/* ignore */
	}
}

function buildAuthenticateRequest(): AuthenticateReq {
	const clientId = resolveClientId();
	const timite = create(TimiteSchema, {
		id: deriveTimiteId(clientId),
		nick: clientId
	});
	const clientInfo = create(ClientInfoSchema, {
		platform: resolvePlatformLabel()
	});
	return create(AuthenticateReqSchema, { timite, clientInfo });
}

const FNV_OFFSET = 1469598103934665603n;
const FNV_PRIME = 1099511628211n;

function deriveTimiteId(input: string): bigint {
	let hash = FNV_OFFSET;
	for (let i = 0; i < input.length; i += 1) {
		hash ^= BigInt(input.charCodeAt(i));
		hash = BigInt.asUintN(64, hash * FNV_PRIME);
	}
	return hash;
}

function resolvePlatformLabel(): string {
	if (!browser) return 'web-ui';
	try {
		return `web-ui:${navigator.userAgent}`;
	} catch {
		return 'web-ui';
	}
}

function buildSessionHeaders(sessionId: string): HeadersInit {
	return [[SESSION_HEADER, sessionId]];
}

function handleAuthFailure(error: unknown): boolean {
	if (error instanceof ConnectError && error.code === Code.Unauthenticated) {
		clearStoredSessionId();
		return true;
	}
	return false;
}

function resolveBackendBaseUrl(): string | null {
	if (!browser) return null;

	const override =
		import.meta.env.VITE_TIM_RPC_URL ??
		import.meta.env.VITE_BACKEND_RPC_URL ??
		import.meta.env.VITE_TIM_HTTP_URL ??
		null;
	if (override) {
		return override.endsWith('/') ? override.slice(0, -1) : override;
	}

	const protocol = window.location.protocol === 'https:' ? 'https' : 'http';
	const host = import.meta.env.VITE_TIM_RPC_HOST ?? window.location.hostname;
	const port = import.meta.env.VITE_TIM_RPC_PORT ?? '8787';
	const rawPath = import.meta.env.VITE_TIM_RPC_PATH ?? '';
	const path = rawPath ? (rawPath.startsWith('/') ? rawPath : `/${rawPath}`) : '';

	return `${protocol}://${host}:${port}${path}`;
}

function generateId() {
	if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
		return crypto.randomUUID();
	}
	return `id-${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
}

function wait(ms: number, signal: AbortSignal): Promise<void> {
	if (typeof window === 'undefined') {
		return new Promise<void>((resolve) => setTimeout(resolve, ms));
	}

	return new Promise<void>((resolve, reject) => {
		const timer = window.setTimeout(() => {
			cleanup();
			resolve();
		}, ms);

		const onAbort = () => {
			cleanup();
			reject(new DOMException('Aborted', 'AbortError'));
		};

		const cleanup = () => {
			window.clearTimeout(timer);
			signal.removeEventListener('abort', onAbort);
		};

		if (signal.aborted) {
			onAbort();
			return;
		}

		signal.addEventListener('abort', onAbort);
	});
}

if (browser) {
	ensureSubscription();
}

export const apiService = {
	async sendMessage(message: string) {
		const trimmed = message.trim();
		if (!trimmed) return;

		try {
			const rpcClient = await ensureClient();
			const sessionId = await ensureSessionId();
			ensureSubscription();
			await rpcClient.sendMessage(
				{
					id: generateId(),
					command: trimmed,
					clientId: resolveClientId()
				},
				{
					headers: buildSessionHeaders(sessionId)
				}
			);
		} catch (error) {
			console.error('Failed to send message to backend', error);
			emitConnectionState('reconnecting');
			handleAuthFailure(error);
		}
	},
	subscribe(listener: ApiListener) {
		listeners.add(listener);
		listener(createConnectionMessage(connectionState));
		ensureSubscription();

		return () => {
			listeners.delete(listener);
			if (listeners.size === 0) {
				stopSubscription();
			}
		};
	},
	getClientId(): string {
		return resolveClientId();
	},
	getSessionId(): Promise<string> {
		return ensureSessionId();
	}
};

export type ApiService = typeof apiService;
