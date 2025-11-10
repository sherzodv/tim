import { browser } from '$app/environment';
import { writable } from 'svelte/store';
import { apiService } from '$lib/api/service';
import type { SpaceUpdateMessage } from '$lib/api/types';
import {
	DEFAULT_HELP,
	DEFAULT_STATUS,
	STORAGE_KEY,
	type MessageEntry,
	type SessionSnapshot,
	type ConnectionState
} from '$lib/models/session';

const CONNECTING_STATUS = 'Connecting to server...';
const CONNECTING_HELP = 'Waiting for backend connection.';
const RECONNECTING_STATUS = 'Reconnecting to server...';
const RECONNECTING_HELP =
	'Backend unreachable. Messaging is disabled until the connection is restored.';

const createDefaultSnapshot = (): SessionSnapshot => ({
	theme: 'night',
	connection: 'connecting',
	entries: [],
	status: CONNECTING_STATUS,
	help: CONNECTING_HELP
});

let entryCounter = 0;
const nextEntryId = () => {
	if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
		return `local-${crypto.randomUUID()}`;
	}
	return `local-${Date.now()}-${entryCounter++}`;
};

const sanitizeEntry = (entry: unknown): MessageEntry | null => {
	if (!entry || typeof entry !== 'object') return null;

	const candidate = entry as {
		id?: unknown;
		senderId?: unknown;
		content?: unknown;
	};
	let id: string;
	if (typeof candidate.id === 'string' && candidate.id.length > 0) {
		id = candidate.id;
	} else if (typeof candidate.id === 'number' && Number.isFinite(candidate.id)) {
		id = `legacy-${candidate.id}`;
	} else {
		id = nextEntryId();
	}
	const senderId =
		typeof candidate.senderId === 'string' && candidate.senderId.length > 0
			? candidate.senderId
			: 'system';
	const content = typeof candidate.content === 'string' ? candidate.content : '';
	if (!content.trim()) {
		return null;
	}

	return {
		id,
		senderId,
		content
	};
};

const sanitizeConnection = (value: unknown): ConnectionState => {
	if (value === 'open' || value === 'reconnecting' || value === 'connecting') {
		return value;
	}
	return 'connecting';
};

const applyConnectionUpdate = (
	state: SessionSnapshot,
	connection: ConnectionState
): SessionSnapshot => {
	switch (connection) {
		case 'open':
			return {
				...state,
				connection,
				status: DEFAULT_STATUS,
				help: DEFAULT_HELP
			};
		case 'connecting':
			return {
				...state,
				connection,
				status: CONNECTING_STATUS,
				help: CONNECTING_HELP
			};
		case 'reconnecting':
		default:
			return {
				...state,
				connection,
				status: RECONNECTING_STATUS,
				help: RECONNECTING_HELP
			};
	}
};

const loadSnapshot = (): SessionSnapshot => {
	if (!browser) return createDefaultSnapshot();

	try {
		const raw = localStorage.getItem(STORAGE_KEY);
		if (!raw) return createDefaultSnapshot();

		const parsed = JSON.parse(raw) as Partial<SessionSnapshot>;
		if (!parsed || typeof parsed !== 'object') return createDefaultSnapshot();

		const sanitizedEntries: MessageEntry[] = Array.isArray(parsed.entries)
			? parsed.entries
					.map((entry) => {
						if (!entry || typeof entry !== 'object') return null;

						const sanitized = sanitizeEntry({
							id: (entry as { id?: unknown }).id,
							senderId: (entry as { senderId?: unknown }).senderId,
							content:
								(entry as { content?: unknown }).content ??
								(entry as { text?: unknown }).text ??
								''
						});
						return sanitized;
					})
					.filter(Boolean) as MessageEntry[]
			: [];

		return {
			...createDefaultSnapshot(),
			...parsed,
			entries: sanitizedEntries,
			connection: sanitizeConnection((parsed as { connection?: unknown }).connection)
		};
	} catch {
		return createDefaultSnapshot();
	}
};

function createSessionStore() {
	const { subscribe: baseSubscribe, update } = writable<SessionSnapshot>(loadSnapshot());

	const persist = (snapshot: SessionSnapshot) => {
		if (!browser) return;

		try {
			localStorage.setItem(STORAGE_KEY, JSON.stringify(snapshot));
		} catch {
			/* ignore storage errors */
		}
	};

	const applyServerMessage = (
		state: SessionSnapshot,
		message: SpaceUpdateMessage
	): SessionSnapshot => {
		switch (message.type) {
			case 'space.message': {
				const entry = sanitizeEntry({
					id: message.id,
					senderId: message.payload.senderId,
					content: message.payload.content
				});
				if (!entry) return state;
				return {
					...state,
					entries: [...state.entries, entry]
				};
			}
			case 'connection.state':
				return applyConnectionUpdate(state, message.payload.state);
			default:
				return state;
		}
	};

	if (browser) {
		apiService.subscribe((message) => {
			update((state) => applyServerMessage(state, message));
		});
	}

	return {
		subscribe(
			run: (value: SessionSnapshot) => void,
			invalidate?: (value?: SessionSnapshot) => void
		) {
			return baseSubscribe((value) => {
				run(value);
				persist(value);
			}, invalidate);
		},
		resetAll() {
			update(() => createDefaultSnapshot());
			if (browser) {
				localStorage.removeItem(STORAGE_KEY);
			}
		}
	};
}

export const session = createSessionStore();
export const DEFAULT_STATUS_TEXT = DEFAULT_STATUS;
export const DEFAULT_HELP_TEXT = DEFAULT_HELP;
