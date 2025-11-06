import { browser } from '$app/environment';
import { writable } from 'svelte/store';
import { apiService } from '$lib/api/service';
import type { SpaceUpdateMessage } from '$lib/api/types';
import {
	DEFAULT_HELP,
	DEFAULT_STATUS,
	STORAGE_KEY,
	THEME_CHOICES,
	type CommandContent,
	type CommandEntry,
	type CommandRole,
	type SessionSnapshot,
	type ConnectionState
} from '$lib/models/session';

const CONNECTING_STATUS = 'Connecting to server...';
const CONNECTING_HELP = 'Waiting for backend connection.';
const RECONNECTING_STATUS = 'Reconnecting to server...';
const RECONNECTING_HELP =
	'Backend unreachable. Commands are disabled until the connection is restored.';

const createDefaultSnapshot = (): SessionSnapshot => ({
	theme: 'night',
	connection: 'connecting',
	entries: [],
	status: CONNECTING_STATUS,
	help: CONNECTING_HELP
});

const sanitizeContent = (content: unknown): CommandContent | null => {
	if (!content || typeof content !== 'object') return null;

	const candidate = content as { kind?: unknown; text?: unknown; html?: unknown };

	if (candidate.kind === 'text' && typeof candidate.text === 'string') {
		return { kind: 'text', text: candidate.text };
	}

	if (candidate.kind === 'html' && typeof candidate.html === 'string') {
		return { kind: 'html', html: candidate.html };
	}

	return null;
};

const sanitizeEntry = (entry: unknown): CommandEntry | null => {
	if (!entry || typeof entry !== 'object') return null;

	const candidate = entry as {
		id?: unknown;
		role?: unknown;
		content?: unknown;
		senderId?: unknown;
	};
	const content = sanitizeContent(candidate.content);
	const role = candidate.role === 'command' || candidate.role === 'output' ? candidate.role : null;
	const id = typeof candidate.id === 'number' ? candidate.id : Date.now();
	const senderId =
		typeof candidate.senderId === 'string' && candidate.senderId.length > 0
			? candidate.senderId
			: 'system';

	if (!content || !role) return null;

	return {
		id,
		role,
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

		const sanitizedEntries: CommandEntry[] = Array.isArray(parsed.entries)
			? parsed.entries
					.map((entry) => {
						if (!entry || typeof entry !== 'object') return null;

						let content = sanitizeContent((entry as { content?: unknown }).content);
						if (!content) {
							const legacyText = (entry as { text?: unknown }).text;
							if (typeof legacyText === 'string') {
								content = { kind: 'text', text: legacyText };
							}
						}
						const role =
							entry.role === 'command' || entry.role === 'output' ? entry.role : 'output';
						const id = typeof entry.id === 'number' ? entry.id : Date.now();
						const senderIdRaw = (entry as { senderId?: unknown }).senderId;
						const senderId =
							typeof senderIdRaw === 'string' && senderIdRaw.length > 0
								? senderIdRaw
								: 'system';

						if (!content) return null;

						return {
							id,
							role,
							senderId,
							content
						};
					})
					.filter(Boolean) as CommandEntry[]
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
			case 'workspace.entries.clear':
				return { ...state, entries: [] };
			case 'workspace.entry.append': {
				const entry = sanitizeEntry(message.payload.entry);
				if (!entry) return state;
				return {
					...state,
					entries: [...state.entries, entry]
				};
			}
			case 'space.message': {
				const entry = sanitizeEntry(message.payload.entry);
				if (!entry) return state;
				return {
					...state,
					entries: [...state.entries, entry]
				};
			}
			case 'session.status':
				return { ...state, status: message.payload.status };
			case 'session.help':
				return { ...state, help: message.payload.help };
			case 'session.theme':
				return state.theme === message.payload.theme
					? state
					: { ...state, theme: message.payload.theme };
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
export const THEME_OPTIONS = THEME_CHOICES;
export const DEFAULT_STATUS_TEXT = DEFAULT_STATUS;
export const DEFAULT_HELP_TEXT = DEFAULT_HELP;
