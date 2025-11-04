import type { CommandEntry, CommandRole, CommandContent, Theme } from '$lib/models/session';

export type CommandRequestMessage = {
	type: 'command.request';
	id: string;
	payload: {
		command: string;
	};
};

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

export type ServerMessage =
	| WorkspaceEntryAppendMessage
	| WorkspaceEntriesClearMessage
	| SessionStatusMessage
	| SessionHelpMessage
	| SessionThemeMessage;

export type ClientMessage = CommandRequestMessage;

export type ApiListener = (message: ServerMessage) => void;

export type OutboxCommand = {
	command: string;
	id: string;
};

export type SocketLike = {
	send: (data: string) => void;
	close: () => void;
};
