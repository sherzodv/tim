export type WorklogItemMessage = {
	kind: 'msg';
	id: bigint;
	author: String;
	content: String;
	time?: string;
};

export type WorklogItemSysMessage = {
	kind: 'sysmsg';
	id: bigint;
	author: String;
	content: String;
	time?: string;
};

export type WorklogItem = WorklogItemMessage | WorklogItemSysMessage;
