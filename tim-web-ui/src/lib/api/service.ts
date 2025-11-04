import { browser } from '$app/environment';
import { connectMockWebSocket } from '$lib/api/mockWebSocket';
import type { ApiListener, ClientMessage, ServerMessage, SocketLike } from '$lib/api/types';

const listeners = new Set<ApiListener>();

let socket: SocketLike | null = null;

const dispatch = (message: ServerMessage) => {
	for (const listener of listeners) {
		listener(message);
	}
};

const createSocket = (listener: ApiListener): SocketLike | null => {
	const backendSocket = browser ? connectBackendSocket(listener) : null;
	return backendSocket ?? connectMockWebSocket(listener);
};

const ensureSocket = () => {
	if (!socket && browser) {
		socket = createSocket(dispatch);
	}
	return socket;
};

const enqueue = (command: string) => {
	const target = ensureSocket();
	if (!target) return;
	const message: ClientMessage = {
		type: 'command.request',
		id: generateId(),
		payload: { command }
	};

	target.send(JSON.stringify(message));
};

const generateId = () => {
	if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
		return crypto.randomUUID();
	}
	return `cmd-${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
};

export const apiService = {
	sendCommand(command: string) {
		enqueue(command);
	},
	subscribe(listener: ApiListener) {
		if (!socket && browser) {
			socket = createSocket(dispatch);
		}
		listeners.add(listener);
		return () => listeners.delete(listener);
	}
};

export type ApiService = typeof apiService;

const connectBackendSocket = (listener: ApiListener): SocketLike | null => {
	const url = resolveBackendSocketUrl();
	if (!url) return null;

	let ws: WebSocket;
	try {
		ws = new WebSocket(url);
	} catch (error) {
		console.warn('Failed to initialize backend WebSocket', error);
		return null;
	}

	const outboundQueue: string[] = [];
	let fallback: SocketLike | null = null;
	let isOpen = false;
	let isClosed = false;

	const flushQueueToBackend = () => {
		if (!isOpen) return;
		while (outboundQueue.length > 0) {
			ws.send(outboundQueue.shift()!);
		}
	};

	const flushQueueToFallback = () => {
		if (!fallback) return;
		while (outboundQueue.length > 0) {
			fallback.send(outboundQueue.shift()!);
		}
	};

	const activateFallback = () => {
		if (fallback) return;
		console.warn('Backend WebSocket unavailable, switching to mock API.');
		fallback = connectMockWebSocket(listener);
		flushQueueToFallback();
		try {
			ws.close();
		} catch {
			/* ignore close errors */
		}
	};

	ws.addEventListener('open', () => {
		if (fallback) return;
		isOpen = true;
		flushQueueToBackend();
	});

	ws.addEventListener('message', (event) => {
		if (fallback) return;

		try {
			const payload = typeof event.data === 'string' ? event.data : String(event.data);
			const message = JSON.parse(payload) as ServerMessage;
			listener(message);
		} catch (error) {
			console.error('Failed to parse backend message', error);
		}
	});

	ws.addEventListener('error', () => {
		if (!isOpen) {
			activateFallback();
		}
	});

	ws.addEventListener('close', () => {
		if (!fallback) {
			isClosed = true;
		}
	});

	return {
		send(data: string) {
			if (fallback) {
				fallback.send(data);
				return;
			}

			if (isClosed) {
				outboundQueue.push(data);
				activateFallback();
				return;
			}

			if (!isOpen) {
				outboundQueue.push(data);
				return;
			}

			try {
				ws.send(data);
			} catch (error) {
				console.error('Failed to send backend message', error);
				outboundQueue.push(data);
				activateFallback();
			}
		},
		close() {
			if (fallback) {
				fallback.close();
				return;
			}

			isClosed = true;
			try {
				ws.close();
			} catch (error) {
				console.error('Failed to close backend socket', error);
			}
		}
	};
};

const resolveBackendSocketUrl = () => {
	if (!browser) return null;

	const override =
		import.meta.env.VITE_TIM_WS_URL ?? import.meta.env.VITE_BACKEND_WS_URL ?? null;
	if (override) return override;

	const protocol = window.location.protocol === 'https:' ? 'wss' : 'ws';
	const host = import.meta.env.VITE_TIM_WS_HOST ?? window.location.hostname;
	const port = import.meta.env.VITE_TIM_WS_PORT ?? '8787';
	const rawPath = import.meta.env.VITE_TIM_WS_PATH ?? '/ws';
	const path = rawPath.startsWith('/') ? rawPath : `/${rawPath}`;

	return `${protocol}://${host}:${port}${path}`;
};
