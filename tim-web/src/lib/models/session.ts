export type Theme = 'night' | 'day';

export type ConnectionState = 'connecting' | 'open' | 'reconnecting';

export type MessageEntry = {
	id: string;
	senderId: string;
	content: string;
};

export type SessionSnapshot = {
	theme: Theme;
	connection: ConnectionState;
	entries: MessageEntry[];
	status: string;
	help: string;
};

export const DEFAULT_STATUS = 'Ready';
export const DEFAULT_HELP =
	'Type a message and press Enter. Press `Esc` to cancel current input.';
export const STORAGE_KEY = 'tim-console-session';
