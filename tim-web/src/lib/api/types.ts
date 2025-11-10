import type { ConnectionState } from '$lib/models/session';

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
		senderId: string;
		content: string;
	};
};

export type SpaceUpdateMessage =
	| ConnectionStateMessage
	| SpaceMessageEvent;

export type ApiListener = (message: SpaceUpdateMessage) => void;
