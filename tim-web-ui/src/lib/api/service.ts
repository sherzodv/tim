import { browser } from '$app/environment';
import type {
	ApiListener,
	ClientMessage,
	ConnectionStateMessage,
	ServerMessage,
	SocketLike
} from '$lib/api/types';
import type { ConnectionState } from '$lib/models/session';

const listeners = new Set<ApiListener>();

let socket: SocketLike | null = null;
let connectionState: ConnectionState = 'connecting';
let connectionEventCounter = 0;

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
	connectionState = state;
	dispatch(createConnectionMessage(state));
};

const ensureSocket = () => {
	if (!browser) return null;
	if (!socket) {
		const backendSocket = connectBackendSocket({
			onMessage: dispatch,
			onConnectionState: emitConnectionState
		});

		const managedSocket: SocketLike = {
			send(data: string) {
				backendSocket.send(data);
			},
			close() {
				backendSocket.close();
				if (socket === managedSocket) {
					socket = null;
					if (connectionState !== 'connecting') {
						emitConnectionState('connecting');
					}
				}
			}
		};

		socket = managedSocket;
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

if (browser) {
	ensureSocket();
}

export const apiService = {
	sendCommand(command: string) {
		enqueue(command);
	},
	subscribe(listener: ApiListener) {
		listeners.add(listener);
		listener(createConnectionMessage(connectionState));
		ensureSocket();
		return () => {
			listeners.delete(listener);
			if (listeners.size === 0 && socket) {
				socket.close();
			}
		};
	}
};

export type ApiService = typeof apiService;

type BackendSocketCallbacks = {
	onMessage: ApiListener;
	onConnectionState: (state: ConnectionState) => void;
};

function connectBackendSocket({
	onMessage,
	onConnectionState
}: BackendSocketCallbacks): SocketLike {
	const outbox: string[] = [];
	let ws: WebSocket | null = null;
	let reconnectTimer: number | null = null;
	let retryCount = 0;
	let manuallyClosed = false;

	const clearReconnectTimer = () => {
		if (reconnectTimer !== null) {
			window.clearTimeout(reconnectTimer);
			reconnectTimer = null;
		}
	};

	const flushOutbox = () => {
		if (!ws || ws.readyState !== WebSocket.OPEN) return;

		while (outbox.length > 0) {
			const payload = outbox.shift()!;

			try {
				ws.send(payload);
			} catch (error) {
				console.error('Failed to send payload over backend socket', error);
				outbox.unshift(payload);
				try {
					ws.close();
				} catch {
					/* ignore close errors */
				}
				return;
			}
		}
	};

	const scheduleReconnect = () => {
		if (manuallyClosed) return;
		if (reconnectTimer !== null) return;

		retryCount = Math.min(retryCount + 1, 10);
		onConnectionState('reconnecting');

		const delay = Math.min(1000 * retryCount, 5000);
		reconnectTimer = window.setTimeout(() => {
			reconnectTimer = null;
			open();
		}, delay);
	};

	const open = () => {
		if (manuallyClosed) return;

		const url = resolveBackendSocketUrl();
		if (!url) {
			console.error('Unable to resolve backend WebSocket URL.');
			scheduleReconnect();
			return;
		}

		const transitionalState: ConnectionState = retryCount === 0 ? 'connecting' : 'reconnecting';
		onConnectionState(transitionalState);

		try {
			ws = new WebSocket(url);
		} catch (error) {
			console.error('Failed to establish backend WebSocket connection', error);
			scheduleReconnect();
			return;
		}

		ws.addEventListener('open', () => {
			retryCount = 0;
			onConnectionState('open');
			flushOutbox();
		});

		ws.addEventListener('message', (event) => {
			try {
				const payload = typeof event.data === 'string' ? event.data : String(event.data);
				const message = JSON.parse(payload) as ServerMessage;
				onMessage(message);
			} catch (error) {
				console.error('Failed to parse backend message', error);
			}
		});

		ws.addEventListener('close', () => {
			ws = null;
			if (!manuallyClosed) {
				scheduleReconnect();
			}
		});

		ws.addEventListener('error', () => {
			if (!manuallyClosed) {
				scheduleReconnect();
			}
		});
	};

	open();

	return {
		send(data: string) {
			if (manuallyClosed) return;

			if (ws && ws.readyState === WebSocket.OPEN) {
				try {
					ws.send(data);
				} catch (error) {
					console.error('Failed to send backend message', error);
					outbox.push(data);
					scheduleReconnect();
					try {
						ws.close();
					} catch {
						/* ignore close errors */
					}
				}
				return;
			}

			outbox.push(data);
		},
		close() {
			manuallyClosed = true;
			clearReconnectTimer();

			if (ws) {
				try {
					ws.close();
				} catch (error) {
					console.error('Failed to close backend socket', error);
				} finally {
					ws = null;
				}
			}
		}
	};
}

function resolveBackendSocketUrl() {
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
}
