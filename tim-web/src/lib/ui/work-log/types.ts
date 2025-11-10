export type WorkLogItemMessage = {
	kind: 'msg';
	id: bigint;
	author: String;
	content: String;
};

export type WorkLogItemSysMessage = {
	kind: 'sysmsg';
	id: bigint;
	author: String;
	content: String;
};

export type WorkLogItem = WorkLogItemMessage | WorkLogItemSysMessage;
