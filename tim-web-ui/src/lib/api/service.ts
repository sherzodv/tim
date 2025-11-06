import { browser } from '$app/environment';
import { createClient, type Client } from '@connectrpc/connect';
import { createGrpcWebTransport } from '@connectrpc/connect-web';
import {
	CommandRole,
	TimApi,
	type CommandContent as RpcCommandContent,
	type CommandEntry as RpcCommandEntry,
	type ServerMessage as RpcServerMessage,
	Theme as RpcTheme
} from '../../gen/tim/api/g1/api_pb';
import type { ApiListener, ConnectionStateMessage, ServerMessage } from '$lib/api/types';
import type {
	CommandContent as UiCommandContent,
	CommandEntry as UiCommandEntry,
	ConnectionState,
	Theme
} from '$lib/models/session';

const CLIENT_ID_STORAGE_KEY = 'tim.client-id';

const listeners = new Set<ApiListener>();
let client: Client<typeof TimApi> | null = null;
let streamController: AbortController | null = null;
let streamTask: Promise<void> | null = null;
let connectionState: ConnectionState = 'connecting';
let connectionEventCounter = 0;
let cachedClientId: string | null = null;

const dispatch = (message: ServerMessage) => {
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
			const stream = rpcClient.subscribe({ clientId: getClientId() }, { signal });
			retryCount = 0;
			emitConnectionState('open');

			for await (const message of stream) {
				if (signal.aborted) break;
				const converted = translateServerMessage(message);
				if (converted) {
					dispatch(converted);
				}
			}

			if (signal.aborted) {
				break;
			}

			emitConnectionState('reconnecting');
		} catch (error) {
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

function translateServerMessage(message: RpcServerMessage): ServerMessage | null {
	const id = message.id || generateId();
	const event = message.event;

	if (!event || !event.case) {
		console.warn('Server message missing event payload', message);
		return null;
	}

	if (event.case === 'spaceNewMessage') {
		const payload = event.value?.message;
		if (!payload?.entry) return null;
		const entry = convertCommandEntry(payload.entry, payload.authorId);
		if (!entry) return null;
		return {
			type: 'space.message',
			id,
			payload: {
				authorId: entry.authorId,
				entry
			}
		};
	}

	if (event.case === 'workspaceEntryAppend' && event.value?.entry) {
		const entry = convertCommandEntry(event.value.entry);
		if (!entry) return null;
		return {
			type: 'workspace.entry.append',
			id,
			payload: { entry }
		};
	}

	if (event.case === 'workspaceEntriesClear') {
		return {
			type: 'workspace.entries.clear',
			id
		};
	}

	if (event.case === 'sessionStatus') {
		return {
			type: 'session.status',
			id,
			payload: {
				status: event.value?.status ?? ''
			}
		};
	}

	if (event.case === 'sessionHelp') {
		return {
			type: 'session.help',
			id,
			payload: {
				help: event.value?.help ?? ''
			}
		};
	}

	if (event.case === 'sessionTheme') {
		const theme = convertTheme(event.value?.theme);
		if (!theme) return null;
		return {
			type: 'session.theme',
			id,
			payload: { theme }
		};
	}

	console.warn('Received unsupported server message payload', message);
	return null;
}

function convertCommandEntry(entry: RpcCommandEntry, authorId?: string): UiCommandEntry | null {
	const roleValue = entry.role ?? CommandRole.UNSPECIFIED;
	const role = roleValue === CommandRole.COMMAND ? 'command' : roleValue === CommandRole.OUTPUT ? 'output' : null;
	if (!role) return null;

	const content = convertCommandContent(entry.content);
	if (!content) return null;

	const rawId = entry.id ?? BigInt(Date.now());
	let id = Number(rawId);
	if (!Number.isFinite(id)) {
		id = Date.now();
	}

	const normalizedAuthor =
		typeof authorId === 'string' && authorId.length > 0 ? authorId : 'system';

	return {
		id,
		role,
		authorId: normalizedAuthor,
		content
	};
}

function convertCommandContent(content?: RpcCommandContent | null): UiCommandContent | null {
	if (!content) return null;
	switch (content.value?.case) {
		case 'text':
			return { kind: 'text', text: content.value.value };
		case 'html':
			return { kind: 'html', html: content.value.value };
		default:
			return null;
	}
}

function convertTheme(value?: RpcTheme | null): Theme | null {
	switch (value) {
		case RpcTheme.DAY:
			return 'day';
		case RpcTheme.NIGHT:
			return 'night';
		default:
			return 'night';
	}
}

function getClientId(): string {
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
	async sendCommand(command: string) {
		const trimmed = command.trim();
		if (!trimmed) return;

		try {
			const rpcClient = await ensureClient();
			ensureSubscription();
			await rpcClient.sendCommand({
				id: generateId(),
				command: trimmed,
				clientId: getClientId()
			});
		} catch (error) {
			console.error('Failed to send command to backend', error);
			emitConnectionState('reconnecting');
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
	}
};

export type ApiService = typeof apiService;
