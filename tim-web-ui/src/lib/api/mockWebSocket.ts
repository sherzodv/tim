import {
	DEFAULT_HELP,
	DEFAULT_STATUS,
	type CommandContent,
	type CommandEntry,
	type CommandRole,
	type Theme
} from '$lib/models/session';
import type {
	ApiListener,
	ClientMessage,
	CommandRequestMessage,
	ServerMessage,
	SocketLike
} from '$lib/api/types';

const BASE_DELAY = 120;

export function connectMockWebSocket(listener: ApiListener): SocketLike {
	let isClosed = false;
	let messageCounter = 0;

	const emit = (message: ServerMessage, delayMultiplier = 0) => {
		if (isClosed) return;
		const delay = BASE_DELAY * delayMultiplier;
		setTimeout(() => {
			if (!isClosed) {
				listener(message);
			}
		}, delay);
	};

	const handleCommand = (message: CommandRequestMessage) => {
		const raw = message.payload.command.trim();
		if (!raw) return;

		const [keyword, ...rest] = raw.split(/\s+/);
		const normalized = keyword.toLowerCase();

		const appendCommand = (delay = 0) =>
			emit(toEntryMessage(message.id, createEntry('command', { kind: 'text', text: raw })), delay);

		switch (normalized) {
			case 'help': {
				appendCommand(0);
				emit(
					toEntryMessage(
						message.id,
						createEntry('output', {
							kind: 'html',
							html: buildHelpHtml()
						})
					),
					1
				);
				emit(toStatusMessage(message.id, 'Help displayed'), 1.5);
				emit(toHelpMessage(message.id, DEFAULT_HELP), 1.6);
				break;
			}

			case 'clear': {
				emit({ type: 'workspace.entries.clear', id: nextEventId(message.id) }, 0.2);
				appendCommand(0.4);
				emit(
					toEntryMessage(
						message.id,
						createEntry('output', {
							kind: 'text',
							text: 'Workspace cleared.'
						})
					),
					0.8
				);
				emit(toStatusMessage(message.id, 'Workspace cleared'), 1.1);
				emit(toHelpMessage(message.id, DEFAULT_HELP), 1.2);
				break;
			}

			case 'theme': {
				const desired = rest[0]?.toLowerCase() as Theme | undefined;
				appendCommand(0);
				if (desired !== 'night' && desired !== 'day') {
					emit(
						toEntryMessage(
							message.id,
							createEntry('output', {
								kind: 'text',
								text: 'Usage: THEME <night|day>'
							})
						),
						1
					);
					emit(toStatusMessage(message.id, 'Theme command incomplete'), 1.3);
					emit(toHelpMessage(message.id, 'Try THEME night or THEME day.'), 1.4);
				} else {
					emit(
						toEntryMessage(
							message.id,
							createEntry('output', {
								kind: 'text',
								text: `Theme set to ${desired}.`
							})
						),
						1
					);
					emit(toThemeMessage(message.id, desired), 1.2);
					emit(toStatusMessage(message.id, `Theme set to ${desired}`), 1.3);
					emit(toHelpMessage(message.id, DEFAULT_HELP), 1.4);
				}
				break;
			}

			case 'reset': {
				appendCommand(0);
				emit({ type: 'workspace.entries.clear', id: nextEventId(message.id) }, 0.8);
				emit(toThemeMessage(message.id, 'night'), 1);
				emit(toStatusMessage(message.id, DEFAULT_STATUS), 1.1);
				emit(toHelpMessage(message.id, DEFAULT_HELP), 1.2);
				break;
			}

			default: {
				appendCommand(0);
				emit(
					toEntryMessage(
						message.id,
						createEntry('output', {
							kind: 'text',
							text: `Unknown command "${raw}". Type HELP to show available commands.`
						})
					),
					1
				);
				emit(toStatusMessage(message.id, 'Unknown command'), 1.3);
				emit(toHelpMessage(message.id, 'Type HELP to see the command list.'), 1.4);
			}
		}
	};

	const send: SocketLike['send'] = (data) => {
		if (isClosed) return;
		let message: ClientMessage;
		try {
			message = JSON.parse(data);
		} catch (error) {
			console.error('MockWebSocket: invalid client message', error);
			return;
		}

		if (message.type === 'command.request') {
			handleCommand(message);
		}
	};

	const close: SocketLike['close'] = () => {
		isClosed = true;
	};

	return { send, close };

	function buildHelpHtml(): string {
		return `
			<div class="help-block">
				<h3>Available commands</h3>
				<dl class="help-list">
					<dt>HELP</dt>
					<dd>Display this help overview.</dd>
					<dt>CLEAR</dt>
					<dd>Reset the workspace log.</dd>
					<dt>THEME &lt;night|day&gt;</dt>
					<dd>Switch the active theme.</dd>
				</dl>
				<p class="help-hint">Commands are case-insensitive. Try “THEME night”.</p>
			</div>
		`.trim();
	}

	function createEntry(role: CommandRole, content: CommandContent): CommandEntry {
		return {
			id: Date.now() + Math.floor(Math.random() * 1000) + messageCounter++,
			role,
			content
		};
	}

	function toEntryMessage(requestId: string, entry: CommandEntry): ServerMessage {
		return {
			type: 'workspace.entry.append',
			id: nextEventId(requestId),
			payload: { entry }
		};
	}

	function toStatusMessage(requestId: string, status: string): ServerMessage {
		return {
			type: 'session.status',
			id: nextEventId(requestId),
			payload: { status }
		};
	}

	function toHelpMessage(requestId: string, help: string): ServerMessage {
		return {
			type: 'session.help',
			id: nextEventId(requestId),
			payload: { help }
		};
	}

	function toThemeMessage(requestId: string, theme: Theme): ServerMessage {
		return {
			type: 'session.theme',
			id: nextEventId(requestId),
			payload: { theme }
		};
	}

	function nextEventId(seed: string): string {
		return `${seed}:${messageCounter++}`;
	}
}
