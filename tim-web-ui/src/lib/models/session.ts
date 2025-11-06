export type Theme = 'night' | 'day';

export type CommandRole = 'command' | 'output';

export type ConnectionState = 'connecting' | 'open' | 'reconnecting';

export type CommandContent =
	| {
			kind: 'text';
			text: string;
	  }
	| {
			kind: 'html';
			html: string;
	  };

export type CommandEntry = {
	id: number;
	role: CommandRole;
	authorId: string;
	content: CommandContent;
};

export type SessionSnapshot = {
	theme: Theme;
	connection: ConnectionState;
	entries: CommandEntry[];
	status: string;
	help: string;
};

export const THEME_CHOICES = [
	{ label: 'Night', value: 'night' as const },
	{ label: 'Day', value: 'day' as const }
] as const;

export const DEFAULT_STATUS = 'Ready';
export const DEFAULT_HELP =
	'Type `HELP` for available commands. Press `Esc` to cancel current input.';
export const STORAGE_KEY = 'tim-console-session';
