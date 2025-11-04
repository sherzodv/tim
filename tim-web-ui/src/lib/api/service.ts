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

if (browser) {
	socket = connectMockWebSocket(dispatch);
}

const enqueue = (command: string) => {
	if (!socket) return;
	const message: ClientMessage = {
		type: 'command.request',
		id: generateId(),
		payload: { command }
	};

	socket.send(JSON.stringify(message));
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
		listeners.add(listener);
		return () => listeners.delete(listener);
	}
};

export type ApiService = typeof apiService;
