import type { CommandEntry, Theme, ConnectionState } from '$lib/models/session';

export type WorkspaceEntryAppendMessage = {
	type: 'workspace.entry.append';
	id: string;
	payload: {
		entry: CommandEntry;
	};
};

export type WorkspaceEntriesClearMessage = {
	type: 'workspace.entries.clear';
	id: string;
};

export type SessionStatusMessage = {
	type: 'session.status';
	id: string;
	payload: {
		status: string;
	};
};

export type SessionHelpMessage = {
	type: 'session.help';
	id: string;
	payload: {
		help: string;
	};
};

export type SessionThemeMessage = {
	type: 'session.theme';
	id: string;
	payload: {
		theme: Theme;
	};
};

export type ConnectionStateMessage = {
	type: 'connection.state';
	id: string;
	payload: {
		state: ConnectionState;
	};
};

export type SpaceMessageEvent = {
	type: 'space.message';
	id: string;
	payload: {
		authorId: string;
		entry: CommandEntry;
	};
};

export type ServerMessage =
	| WorkspaceEntryAppendMessage
	| WorkspaceEntriesClearMessage
	| SessionStatusMessage
	| SessionHelpMessage
	| SessionThemeMessage
	| ConnectionStateMessage
	| SpaceMessageEvent;

export type ApiListener = (message: ServerMessage) => void;
